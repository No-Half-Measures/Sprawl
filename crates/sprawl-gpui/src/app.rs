use crate::{
    format::{age, count, format_size, middle_path, percent},
    layout::{Node, hit},
    state::{RequestTag, coalesce},
    treemap::{self, MapCache},
};
use futures::{StreamExt, channel::mpsc};
use gpui::{prelude::*, *};
use sprawl_core::{
    Backend, BackendEvent,
    backend::Drive,
    cache::SavedScan,
    scanner::{Kind, Progress},
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

enum Screen {
    Home,
    Scan,
    Results,
}

pub struct DiskApp {
    backend: Backend,
    _event_task: Task<()>,
    pending: Vec<BackendEvent>,
    screen: Screen,
    drives: Vec<Drive>,
    saved: Vec<SavedScan>,
    busy: bool,
    cancelling: bool,
    scan: Option<u64>,
    progress: Progress,
    error: String,
    trail: Vec<Node>,
    history: Vec<Vec<Node>>,
    tree: Option<Rc<Node>>,
    requests: RequestTag,
    version: u64,
    cache: Rc<RefCell<MapCache>>,
    hover: Option<usize>,
    pointer: Point<Pixels>,
    allocated: bool,
    skipped: u64,
    focus: FocusHandle,
    trace: bool,
    startup: Instant,
}

impl DiskApp {
    pub fn new(
        data: PathBuf,
        open: Option<String>,
        startup: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, mut events) = mpsc::unbounded();
        let backend = Backend::new(data, None, move |event| {
            let _ = tx.unbounded_send(event);
        });
        let event_task = cx.spawn(async move |weak, cx| {
            while let Some(event) = events.next().await {
                if weak
                    .update(cx, |this, cx| {
                        let needs_notify = this.pending.is_empty();
                        this.pending.push(event);
                        while let Ok(event) = events.try_recv() {
                            this.pending.push(event);
                        }
                        // One repaint request per frame, even during a progress flood.
                        if needs_notify {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut this = Self {
            backend,
            _event_task: event_task,
            pending: Vec::new(),
            screen: Screen::Home,
            drives: vec![],
            saved: vec![],
            busy: true,
            cancelling: false,
            scan: None,
            progress: Progress::default(),
            error: String::new(),
            trail: vec![],
            history: vec![],
            tree: None,
            requests: RequestTag::default(),
            version: 0,
            cache: Rc::default(),
            hover: None,
            pointer: point(px(0.), px(0.)),
            allocated: false,
            skipped: 0,
            focus: cx.focus_handle(),
            trace: std::env::var_os("SPRAWL_TRACE").is_some(),
            startup,
        };
        this.refresh(open, cx);
        this
    }

    fn work<R: Send + 'static>(
        &self,
        job: impl FnOnce(Backend) -> R + Send + 'static,
        done: impl FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let backend = self.backend.clone();
        let task = cx.background_executor().spawn(async move { job(backend) });
        cx.spawn(async move |weak, cx| {
            let result = task.await;
            let _ = weak.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh(&mut self, open: Option<String>, cx: &mut Context<Self>) {
        let initial = self.busy && matches!(self.screen, Screen::Home);
        self.work(
            |b| (b.list_drives(), b.list_saved_scans()),
            move |this, (drives, saved), cx| {
                this.drives = drives;
                if initial {
                    this.busy = false;
                }
                match saved {
                    Ok(saved) => this.saved = saved,
                    Err(e) => {
                        this.error = e;
                        return;
                    }
                }
                if let Some(path) = open {
                    if let Some(saved) = this
                        .saved
                        .iter()
                        .find(|s| same_path(&s.path, &path))
                        .cloned()
                    {
                        this.open(saved, cx);
                    } else {
                        this.fail("No saved scan for the requested path.".into());
                    }
                }
            },
            cx,
        );
    }

    fn fail(&mut self, error: String) {
        self.error = error;
        self.busy = false;
        self.scan = None;
        self.requests.next();
        self.screen = Screen::Home;
    }

    fn drain(&mut self, cx: &mut Context<Self>) {
        // A very fast scan can finish before start_scan's return reaches the foreground.
        if matches!(self.screen, Screen::Scan) && self.busy {
            return;
        }
        let (progress, events) = coalesce(self.pending.drain(..), self.scan);
        if let Some(progress) = progress {
            self.progress = progress;
            cx.notify();
        }
        for event in events {
            match event {
                BackendEvent::ScanComplete(e) => {
                    self.scan = None;
                    self.allocated = e.data.has_allocated;
                    self.skipped = e.data.errors;
                    self.trail = vec![e.data.root.into()];
                    self.history.clear();
                    self.request_tree(cx);
                }
                BackendEvent::ScanCancelled(_) => {
                    self.scan = None;
                    self.cancelling = false;
                    self.screen = Screen::Home;
                    self.refresh(None, cx);
                }
                BackendEvent::ScanError(e) => self.fail(e.data.message),
                BackendEvent::SavedScansChanged => self.refresh(None, cx),
                _ => {}
            }
            cx.notify();
        }
    }

    fn start(&mut self, path: String, cx: &mut Context<Self>) {
        if self.busy || matches!(self.screen, Screen::Scan) {
            return;
        }
        self.requests.next();
        self.tree = None;
        self.hover = None;
        self.error.clear();
        self.screen = Screen::Scan;
        self.busy = true;
        self.cancelling = false;
        self.progress = Progress::default();
        self.work(
            move |b| b.start_scan(path),
            |this, result, _| {
                this.busy = false;
                match result {
                    Ok(id) => this.scan = Some(id),
                    Err(e) => this.fail(e),
                }
            },
            cx,
        );
    }

    fn pick(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a folder to map".into()),
        });
        cx.spawn(async move |weak, cx| {
            let result = prompt.await;
            let _ = weak.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            this.start(path.to_string_lossy().into_owned(), cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => this.fail(e.to_string()),
                    Err(e) => this.fail(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, saved: SavedScan, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error.clear();
        self.requests.next();
        self.work(
            move |b| b.load_saved_scan(saved.path, saved.scanned_at),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(scan) => {
                        this.allocated = scan.has_allocated;
                        this.skipped = scan.errors;
                        this.trail = vec![scan.root.into()];
                        this.history.clear();
                        this.request_tree(cx);
                    }
                    Err(e) => this.fail(e),
                }
            },
            cx,
        );
    }

    fn request_tree(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.trail.last().and_then(|n| n.id) else {
            return;
        };
        let tag = self.requests.next();
        self.hover = None;
        self.busy = true;
        self.tree = None;
        self.screen = Screen::Results;
        self.work(
            move |b| b.get_tree(Some(id), 3, 100),
            move |this, result, _| {
                if !this.requests.accepts(tag) {
                    return;
                }
                this.busy = false;
                match result {
                    Ok(tree) => {
                        this.tree = Some(Rc::new(tree.into()));
                        this.version += 1;
                    }
                    Err(e) => this.fail(e),
                }
            },
            cx,
        );
    }

    fn navigate(&mut self, next: Vec<Node>, cx: &mut Context<Self>) {
        self.history.push(std::mem::replace(&mut self.trail, next));
        self.request_tree(cx);
    }
    fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(trail) = self.history.pop() {
            self.trail = trail;
            self.request_tree(cx);
        }
    }
    fn up(&mut self, cx: &mut Context<Self>) {
        if self.trail.len() > 1 {
            self.navigate(self.trail[..self.trail.len() - 1].to_vec(), cx);
        }
    }
    fn enter(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let node = self
            .hover
            .and_then(|i| self.cache.borrow().tiles.get(i).map(|t| t.node.clone()));
        if let Some(node) = node
            && node.kind == Kind::Dir
            && let Some(id) = node.id
            && let Some(tree) = &self.tree
            && let Some(path) = find_path(tree, id)
        {
            let mut next = self.trail.clone();
            next.extend(path.into_iter().skip(1));
            self.navigate(next, cx);
        }
    }

    fn button(
        &self,
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x293747))
            .border_1()
            .border_color(rgb(0x364454))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x304156)))
            .child(text.into())
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this, cx);
                cx.notify();
            }))
    }

    fn home(&self, cx: &Context<Self>) -> AnyElement {
        let mut body = div()
            .id("home-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p_8()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(format!("Your drives · {} available", self.drives.len()))
                    .child(self.button("pick", "Scan a folder…", |this, cx| this.pick(cx), cx)),
            );
        if !self.error.is_empty() {
            body = body.child(div().text_color(rgb(0xf4b3b3)).child(self.error.clone()));
        }
        if self.busy {
            body = body.child("Opening…");
        }
        let mut drives = div().flex().flex_wrap().gap_3();
        for (index, d) in self.drives.iter().enumerate() {
            let saved = self.saved.iter().find(|s| same_path(&s.path, &d.mount));
            let mount = d.mount.clone();
            let mut card = div()
                .w(px(330.))
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x364454))
                .bg(rgb(0x202c39))
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_lg().child(if d.label.is_empty() {
                    "Local disk".into()
                } else {
                    d.label.clone()
                }))
                .child(d.mount.clone())
                .child(
                    div()
                        .h(px(7.))
                        .w_full()
                        .bg(rgb(0x364454))
                        .rounded_sm()
                        .child(
                            div()
                                .h_full()
                                .w(relative(if d.total > 0 {
                                    d.total.saturating_sub(d.free) as f32 / d.total as f32
                                } else {
                                    0.
                                }))
                                .bg(rgb(0x86b9ef)),
                        ),
                )
                .child(format!(
                    "{} used of {}",
                    format_size(d.total.saturating_sub(d.free) as f64),
                    format_size(d.total as f64)
                ))
                .child(
                    div()
                        .text_color(rgb(0xa5b4c5))
                        .child(format!("{} free", format_size(d.free as f64))),
                );
            let mut actions = div().flex().gap_2().justify_end();
            if let Some(saved) = saved {
                card = card.child(div().text_sm().text_color(rgb(0xa5b4c5)).child(format!(
                    "Last scanned {} · {}",
                    age(saved.scanned_at),
                    format_size(saved.size as f64)
                )));
                let saved = saved.clone();
                actions = actions.child(self.button(
                    ("open-drive", index),
                    "Open last scan",
                    move |this, cx| this.open(saved.clone(), cx),
                    cx,
                ));
            }
            actions = actions.child(self.button(
                ("scan-drive", index),
                "Scan drive",
                move |this, cx| this.start(mount.clone(), cx),
                cx,
            ));
            drives = drives.child(card.child(actions));
        }
        body = body.child(drives);
        if self.drives.is_empty() && !self.busy {
            body = body.child("No drives found. Choose a folder to explore its disk usage.");
        }
        let folders: Vec<_> = self
            .saved
            .iter()
            .filter(|s| !self.drives.iter().any(|d| same_path(&s.path, &d.mount)))
            .collect();
        if !folders.is_empty() {
            body = body.child(div().mt_4().child("Recent folders"));
        }
        for (index, saved) in folders.into_iter().enumerate() {
            let saved = saved.clone();
            body = body.child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_3()
                    .bg(rgb(0x202c39))
                    .rounded_md()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(middle_path(&saved.path, 90))
                            .child(div().text_sm().text_color(rgb(0xa5b4c5)).child(format!(
                                "{} · {}",
                                age(saved.scanned_at),
                                format_size(saved.size as f64)
                            ))),
                    )
                    .child(self.button(
                        ("open-folder", index),
                        "Open",
                        move |this, cx| this.open(saved.clone(), cx),
                        cx,
                    )),
            );
        }
        body.into_any_element()
    }

    fn scanning(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_5()
            .child(div().text_3xl().child("Mapping your space"))
            .child("Finding the pieces that make up this folder.")
            .child(div().text_3xl().child(format!(
                "{} found       {}",
                count(self.progress.files, "file"),
                format_size(self.progress.bytes as f64)
            )))
            .child(
                div().w(px(440.)).h(px(4.)).bg(rgb(0x364454)).child(
                    div().w(px(140.)).h_full().bg(rgb(0x86b9ef)).with_animation(
                        "scan-pulse",
                        Animation::new(Duration::from_secs(1))
                            .repeat()
                            .with_easing(pulsating_between(0.2, 1.)),
                        |element, delta| element.opacity(delta),
                    ),
                ),
            )
            .child(
                div()
                    .text_sm()
                    .child(middle_path(&self.progress.current_path, 90)),
            )
            .child(format!(
                "{} · {}",
                count(self.progress.folders, "folder"),
                count(self.progress.errors, "unreadable item")
            ))
            .child(self.button(
                "cancel",
                if self.cancelling {
                    "Cancelling…"
                } else {
                    "Cancel scan"
                },
                |this, cx| {
                    if this.cancelling {
                        return;
                    }
                    this.cancelling = true;
                    this.work(
                        |b| b.cancel_scan(),
                        |this, result, _| {
                            if let Err(e) = result {
                                this.fail(e);
                            }
                        },
                        cx,
                    );
                },
                cx,
            ))
            .into_any_element()
    }

    fn results(&self, cx: &Context<Self>) -> AnyElement {
        let mut crumbs = div()
            .flex()
            .gap_2()
            .items_center()
            .child(self.button("back", "←", |this, cx| this.back(cx), cx))
            .child(self.button("up", "↑", |this, cx| this.up(cx), cx));
        for (index, node) in self.trail.iter().enumerate() {
            crumbs = crumbs.child(self.button(
                ("crumb", index),
                node.name.clone(),
                move |this, cx| {
                    if index + 1 < this.trail.len() {
                        this.navigate(this.trail[..=index].to_vec(), cx);
                    }
                },
                cx,
            ));
        }
        let mut body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(crumbs);
        if let Some(node) = self.trail.last() {
            body = body.child(format!(
                "{} · {}",
                format_size(node.size as f64),
                count(node.files, "file")
            ));
        }
        if self.skipped > 0 {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xf2c97d))
                    .child(format!("{} couldn't be read.", count(self.skipped, "item"))),
            );
        }
        let mut surface = div()
            .id("treemap")
            .relative()
            .flex_1()
            .min_h_0()
            .bg(rgb(0x131c26))
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let cache = this.cache.borrow();
                let position = event.position - cache.bounds.origin;
                let hover = hit(
                    &cache.tiles,
                    f32::from(position.x) as f64,
                    f32::from(position.y) as f64,
                );
                drop(cache);
                this.pointer = position;
                this.hover = hover;
                cx.notify();
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.hover = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                if let ClickEvent::Mouse(event) = event
                    && event.down.button == MouseButton::Left
                    && event.up.button == MouseButton::Left
                {
                    let cache = this.cache.borrow();
                    let position = event.up.position - cache.bounds.origin;
                    let pressed = event.down.position - cache.bounds.origin;
                    let hover = hit(
                        &cache.tiles,
                        f32::from(position.x) as f64,
                        f32::from(position.y) as f64,
                    );
                    // GPUI checks the whole surface; also require the same tile.
                    if hover
                        != hit(
                            &cache.tiles,
                            f32::from(pressed.x) as f64,
                            f32::from(pressed.y) as f64,
                        )
                    {
                        return;
                    }
                    drop(cache);
                    this.hover = hover;
                    this.enter(cx);
                }
            }));
        if let Some(tree) = &self.tree {
            surface = surface.child(treemap::map(
                tree.clone(),
                self.version,
                self.cache.clone(),
                self.hover,
                self.trace,
                self.startup,
            ));
        } else {
            surface = surface.child("Loading…");
        }
        if let Some(tile) = self
            .hover
            .and_then(|i| self.cache.borrow().tiles.get(i).cloned())
        {
            let cache = self.cache.borrow();
            let left = f32::from(self.pointer.x)
                .min((f32::from(cache.bounds.size.width) - 265.).max(4.))
                .max(4.);
            let top = f32::from(self.pointer.y)
                .min((f32::from(cache.bounds.size.height) - 135.).max(4.))
                .max(4.);
            let mut tip = div()
                .absolute()
                .left(px(left + 14.))
                .top(px(top + 14.))
                .w(px(250.))
                .p_3()
                .rounded_md()
                .bg(rgb(0x202c39))
                .border_1()
                .border_color(rgb(0x364454))
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child(tile.node.name.clone())
                .child(format!(
                    "{} · {}",
                    format_size(tile.node.size as f64),
                    percent(
                        tile.node.size as f64,
                        self.tree.as_ref().map_or(0., |t| t.size as f64)
                    )
                ));
            if self.allocated {
                tip = tip.child(format!(
                    "{} on disk",
                    format_size(tile.node.allocated as f64)
                ));
            }
            tip = tip.child(format!(
                "{} · {}",
                count(tile.node.files, "file"),
                count(tile.node.folders, "folder")
            ));
            if tile.node.id.is_none() {
                tip = tip.child("Zoom into the containing folder for more detail.");
            }
            surface = surface.child(tip);
        }
        body.child(surface).into_any_element()
    }
}

impl Render for DiskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain(cx);
        if !self.focus.is_focused(window) {
            self.focus.focus(window);
        }
        let content = match self.screen {
            Screen::Home => self.home(cx),
            Screen::Scan => self.scanning(cx),
            Screen::Results => self.results(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x18212c))
            .text_color(rgb(0xeaf0f6))
            .font_family("Segoe UI")
            .text_sm()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.hover = None;
                    cx.notify();
                } else if matches!(this.screen, Screen::Results)
                    && (event.keystroke.key == "backspace"
                        || (event.keystroke.modifiers.alt && event.keystroke.key == "left"))
                {
                    this.back(cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px_5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(0x364454))
                    .child(div().text_lg().child("Sprawl"))
                    .child(self.button(
                        "home",
                        "Home",
                        |this, cx| {
                            if !this.busy && !matches!(this.screen, Screen::Scan) {
                                this.requests.next();
                                this.busy = false;
                                this.screen = Screen::Home;
                                this.hover = None;
                                this.refresh(None, cx);
                            }
                        },
                        cx,
                    )),
            )
            .child(content)
    }
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim_end_matches(['\\', '/']))
}
fn find_path(tree: &Node, id: usize) -> Option<Vec<Node>> {
    if tree.id == Some(id) {
        return Some(vec![tree.clone()]);
    }
    for child in tree.children.iter().flatten() {
        if let Some(mut path) = find_path(child, id) {
            path.insert(0, tree.clone());
            return Some(path);
        }
    }
    None
}
