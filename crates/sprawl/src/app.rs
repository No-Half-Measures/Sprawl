#[path = "header.rs"]
mod header;
#[path = "integration.rs"]
mod integration;
#[path = "map_panel.rs"]
mod map_panel;
#[path = "panels.rs"]
mod panels;
use crate::input::{Changed, TextInput};
use crate::preferences::{ColorBy, Preferences};
use crate::{
    format::{age, count, format_size, middle_path, percent},
    layout::Node,
    state::{RequestTag, coalesce},
    treemap::MapCache,
};
use futures::{StreamExt, channel::mpsc};
use gpui::{prelude::*, *};
use panels::Panels;
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
    palette: crate::theme::Palette,
    backend: Backend,
    _event_task: Task<()>,
    pending: Vec<BackendEvent>,
    screen: Screen,
    drives: Vec<Drive>,
    saved: Vec<SavedScan>,
    busy: bool,
    opening: bool,
    prompt_open: bool,
    pending_launch: Option<String>,
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
    pointer_tile: Option<usize>,
    linked_hover: Option<usize>,
    filter: Option<String>,
    panels: Panels,
    search_input: Entity<TextInput>,
    files_input: Entity<TextInput>,
    modified: bool,
    snapshot: Option<SavedScan>,
    scan_path: String,
    prefs: Preferences,
    prefs_path: PathBuf,
    details_bounds: Rc<RefCell<Bounds<Pixels>>>,
    dragging: bool,
    allocated: bool,
    skipped: u64,
    focus: FocusHandle,
    trace: bool,
    startup: Instant,
    restore_focus: Option<FocusHandle>,
    session: u64,
    notice: String,
    notice_serial: u64,
    menu: Option<(Node, Point<Pixels>, Size<Pixels>)>,
    deleting: Option<integration::DeleteDialog>,
    delete_pending: bool,
    exporting: bool,
    admin: sprawl_core::backend::AdminStatus,
    admin_busy: bool,
    settings_open: bool,
    recycled: u64,
    deleted: u64,
}

impl DiskApp {
    pub fn set_palette(&mut self, palette: crate::theme::Palette, cx: &mut Context<Self>) {
        if self.palette != palette {
            self.palette = palette;
            cx.set_global(palette);
            self.cache.borrow_mut().clear();
            cx.refresh_windows();
            cx.notify();
        }
    }

    pub fn new(
        data: PathBuf,
        open: Option<String>,
        launch: Option<String>,
        launches: Option<mpsc::UnboundedReceiver<Vec<String>>>,
        startup: Instant,
        window_handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let prefs_path = data.join("panels.json");
        let prefs = Preferences::load(&prefs_path);
        let search_input =
            cx.new(|cx| TextInput::new("Search\u{2026} *.iso size>1GB type:video", cx));
        let files_input = cx.new(|cx| TextInput::new("Filter\u{2026} *.log age>1y", cx));
        cx.subscribe(&search_input, |this, _, _: &Changed, cx| {
            this.search_changed(cx)
        })
        .detach();
        cx.subscribe(&files_input, |this, _, _: &Changed, cx| {
            this.files_changed(cx)
        })
        .detach();
        let (tx, mut events) = mpsc::unbounded();
        let backend = Backend::new(data, launch, move |event| {
            let _ = tx.unbounded_send(event);
        });
        if let Some(mut launches) = launches {
            cx.spawn(async move |weak, cx| {
                while let Some(args) = launches.next().await {
                    let path = crate::actions::arguments(args.into_iter().skip(1))
                        .ok()
                        .and_then(|args| sprawl_core::shell::launch_path(args.remaining));
                    let _ = window_handle.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |this: &mut Self, cx| {
                            if this.modal() {
                                return;
                            }
                            window.activate_window();
                            if let Some(path) = path {
                                this.backend.request_scan(path);
                            }
                            this.drain(cx);
                            window.refresh();
                            cx.notify();
                        });
                    });
                }
            })
            .detach();
        }
        #[cfg(windows)]
        let mut startup_error = String::new();
        #[cfg(not(windows))]
        let startup_error = String::new();
        #[cfg(windows)]
        {
            let status = backend.admin_status();
            if status.always_elevate
                && !status.elevated
                && !std::env::args_os().any(|a| a == "--elevated")
            {
                match backend.restart_as_admin() {
                    Ok(()) => std::process::exit(0),
                    Err(_) => backend.mark_admin_declined(),
                }
            }
            if status.elevated
                && let Err(e) =
                    sprawl_core::shell::allow_scan_requests(crate::single_instance::IDENTIFIER)
            {
                startup_error = e;
            }
        }
        let admin = backend.admin_status();
        let event_task = cx.spawn(async move |weak, cx| {
            while let Some(event) = events.next().await {
                if weak
                    .update(cx, |this, cx| {
                        let needs_notify = this.pending.is_empty();
                        if matches!(event, BackendEvent::ScanRequested(_)) && this.modal() {
                            return;
                        }
                        this.pending.push(event);
                        while let Ok(event) = events.try_recv() {
                            if matches!(event, BackendEvent::ScanRequested(_)) && this.modal() {
                                continue;
                            }
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
            palette: crate::theme::palette(cx),
            backend,
            _event_task: event_task,
            pending: Vec::new(),
            screen: Screen::Home,
            drives: vec![],
            saved: vec![],
            busy: true,
            opening: false,
            prompt_open: false,
            pending_launch: None,
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
            pointer_tile: None,
            linked_hover: None,
            filter: None,
            panels: Panels::new(),
            search_input,
            files_input,
            modified: false,
            snapshot: None,
            scan_path: String::new(),
            prefs,
            prefs_path,
            details_bounds: Rc::default(),
            dragging: false,
            allocated: false,
            skipped: 0,
            focus: cx.focus_handle(),
            trace: std::env::var_os("SPRAWL_TRACE").is_some(),
            startup,
            restore_focus: None,
            session: 0,
            notice: String::new(),
            notice_serial: 0,
            menu: None,
            deleting: None,
            delete_pending: false,
            exporting: false,
            admin,
            admin_busy: false,
            settings_open: false,
            recycled: 0,
            deleted: 0,
        };
        this.error = startup_error;
        if this.admin.declined {
            this.notice("Continuing without administrator access.".into(), cx);
        }
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

    fn work_on_own_thread<R: Send + 'static>(
        &self,
        job: impl FnOnce(Backend) -> Result<R, String> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<R, String>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        // Shell operations need a fresh COM apartment; GPUI's Windows pool is already MTA.
        let backend = self.backend.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        let thread = std::thread::Builder::new().spawn(move || {
            let _ = sender.send(job(backend));
        });
        cx.spawn(async move |weak, cx| {
            let result = match thread {
                Ok(_) => receiver.await.unwrap_or_else(|_| {
                    Err("The operation's worker thread stopped unexpectedly.".into())
                }),
                Err(e) => Err(format!("Couldn't start the operation's worker thread: {e}")),
            };
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
                this.drives
                    .sort_by(|a, b| crate::actions::compare_mounts(&a.mount, &b.mount));
                if initial {
                    this.busy = false;
                }
                match saved {
                    Ok(saved) => this.saved = saved,
                    Err(e) => this.error = e,
                }
                if let Some(path) = this.backend.take_launch_path() {
                    this.scan_requested(path, cx);
                } else if let Some(path) = open {
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
        let (progress, events) = coalesce(self.pending.drain(..), self.scan);
        if let Some(progress) = progress {
            self.progress = progress;
            cx.notify();
        }
        for event in events {
            if !crate::state::accepts_event(&event, self.scan) {
                continue;
            }
            match event {
                BackendEvent::ScanComplete(e) => {
                    self.menu = None;
                    self.scan = None;
                    self.allocated = e.data.has_allocated;
                    self.modified = e.data.has_modified;
                    self.skipped = e.data.errors;
                    self.trail = vec![e.data.root.into()];
                    self.history.clear();
                    self.request_tree(cx);
                }
                BackendEvent::ScanCancelled(_) => {
                    self.scan = None;
                    self.cancelling = false;
                    self.screen = Screen::Home;
                    self.notice(
                        "Scan cancelled. Choose a drive or folder to start again.".into(),
                        cx,
                    );
                    self.refresh(None, cx);
                }
                BackendEvent::ScanError(e) => self.fail(e.data.message),
                BackendEvent::SavedScansChanged => self.refresh(None, cx),
                BackendEvent::ScanRequested(path) => self.scan_requested(path, cx),
                BackendEvent::ScanProgress(_) => {}
            }
            cx.notify();
        }
    }

    fn start(&mut self, path: String, cx: &mut Context<Self>) {
        if self.modal() || self.opening || self.prompt_open {
            return;
        }
        self.session += 1;
        self.menu = None;
        self.settings_open = false;
        self.notice.clear();
        self.recycled = 0;
        self.deleted = 0;
        self.requests.next();
        self.panels.invalidate();
        self.snapshot = None;
        self.scan_path = path.clone();
        self.tree = None;
        self.cache.borrow_mut().clear();
        self.pointer_tile = None;
        self.hover = None;
        self.error.clear();
        self.screen = Screen::Scan;
        self.cancelling = false;
        self.progress = Progress::default();
        self.scan = None;
        self.busy = false;
        match self.backend.start_scan(path) {
            Ok(id) => self.scan = Some(id),
            Err(e) => self.fail(e),
        }
        cx.notify();
    }

    fn pick(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.prompt_open = true;
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
                this.prompt_open = false;
                // A forwarded launch takes precedence over the folder picker result.
                if this.resume_launch(cx) {
                    cx.notify();
                    return;
                }
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
        if self.modal() || self.busy {
            return;
        }
        self.session += 1;
        self.cache.borrow_mut().clear();
        self.pointer_tile = None;
        self.menu = None;
        self.settings_open = false;
        self.notice.clear();
        self.tree = None;
        self.scan = None;
        self.busy = true;
        self.opening = true;
        self.error.clear();
        self.requests.next();
        let session = self.session;
        self.work(
            move |b| b.load_saved_scan(saved.path, saved.scanned_at),
            move |this, result, cx| {
                if this.session != session {
                    return;
                }
                this.busy = false;
                this.opening = false;
                // Loading bumps the backend generation; launches must wait for it.
                if this.resume_launch(cx) {
                    return;
                }
                match result {
                    Ok(scan) => {
                        this.allocated = scan.has_allocated;
                        this.modified = scan.has_modified;
                        this.scan_path = scan.saved.path.clone();
                        this.recycled = scan.saved.recycled;
                        this.deleted = scan.saved.deleted;
                        this.snapshot = Some(scan.saved);
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
        self.set_hover(None, cx);
        self.pointer_tile = None;
        self.filter = None;
        self.panels.reset();
        self.busy = true;
        self.tree = None;
        self.cache.borrow_mut().clear();
        self.pointer_tile = None;
        self.error.clear();
        self.screen = Screen::Results;
        self.work(
            move |b| -> Result<_, String> {
                Ok((
                    b.get_tree(Some(id), 3, 100)?,
                    b.get_largest_files(id, 50)?,
                    b.get_type_breakdown(id)?,
                ))
            },
            move |this, result, cx| {
                if !this.requests.accepts(tag) {
                    return;
                }
                this.busy = false;
                match result {
                    Ok((tree, files, types)) => {
                        this.cache.borrow_mut().clear();
                        this.pointer_tile = None;
                        this.tree = Some(Rc::new(tree.into()));
                        this.panels.largest =
                            files.into_iter().map(panels::FileRow::from).collect();
                        this.panels.breakdown = types;
                        this.version += 1;
                    }
                    Err(e) => this.error = e,
                }
                this.search_changed(cx);
                this.load_tab(cx);
            },
            cx,
        );
        self.work(
            move |b| b.get_changes(id),
            move |this, result, _| {
                if !this.requests.accepts(tag) {
                    return;
                }
                match result {
                    Ok(changes) => {
                        this.panels.grown = changes
                            .as_ref()
                            .map(|c| c.grown.iter().map(panels::FileRow::grown).collect())
                            .unwrap_or_default();
                        this.panels.changes = changes;
                    }
                    Err(e) => this.error = e,
                }
            },
            cx,
        );
    }

    fn navigate(&mut self, next: Vec<Node>, cx: &mut Context<Self>) {
        if self.modal() {
            return;
        }
        self.history.push(std::mem::replace(&mut self.trail, next));
        self.request_tree(cx);
    }
    fn back(&mut self, cx: &mut Context<Self>) {
        if self.modal() {
            return;
        }
        if let Some(trail) = self.history.pop() {
            self.trail = trail;
            self.request_tree(cx);
        }
    }
    fn up(&mut self, cx: &mut Context<Self>) {
        if self.modal() {
            return;
        }
        if self.trail.len() > 1 {
            self.navigate(self.trail[..self.trail.len() - 1].to_vec(), cx);
        }
    }
    fn enter(&mut self, cx: &mut Context<Self>) {
        if self.modal() {
            return;
        }
        if self.busy {
            return;
        }
        let node = self.hover.and_then(|id| {
            self.tree
                .as_ref()
                .and_then(|t| find_path(t, id))
                .and_then(|p| p.last().cloned())
        });
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
        enabled: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let palette = crate::theme::palette(cx);
        let text = text.into();
        let enabled = enabled
            && !self.busy
            && !self.modal()
            && !(matches!(self.screen, Screen::Home) && self.admin_busy);
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(palette.raised)
            .border_1()
            .border_color(palette.line)
            .when(!enabled, |d| d.opacity(0.4))
            .when(enabled, |d| {
                d.cursor_pointer().hover(|s| s.bg(palette.hover))
            })
            .child(text)
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    action(this, cx);
                    cx.notify();
                }
            }))
    }

    fn home(&self, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let mut body = div()
            .id("home-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p_8()
            .flex()
            .flex_col()
            .gap_4();
        let mut section = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_x(px(14.))
                            .gap_y(px(6.))
                            .child(format!("Your drives · {} available", self.drives.len()))
                            .when(self.admin.supported && self.admin.elevated, |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(palette.muted)
                                        .child("Running as administrator \u{b7} drive scans use the fast path."),
                                )
                            }),
                    )
                    .child(self.button(
                        "pick",
                        "Scan a folder…",
                        true,
                        |this, cx| this.pick(cx),
                        cx,
                    )),
            )
            .when(self.admin.supported && !self.admin.elevated, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_4()
                        .gap_y(px(10.))
                        .pt_1()
                        .pb_2()
                        .text_size(px(13.))
                        .text_color(palette.muted)
                        .child(
                            div()
                                .flex_grow()
                                .flex_basis(px(300.))
                                .child("Drive scans are much faster when Sprawl runs as administrator."),
                        )
                        .child(
                            self.button(
                                "admin-restart",
                                "Restart as administrator",
                                !self.admin_busy,
                                |this, cx| this.restart_admin(cx),
                                cx,
                            )
                            .px(px(10.))
                            .py(px(6.))
                            .text_color(palette.text),
                        ),
                )
            });

        if self.busy {
            section = section.child("Opening…");
        }
        let mut drives = div().mt_2().flex().flex_wrap().gap_3();
        for (index, d) in self.drives.iter().enumerate() {
            let saved = self.saved.iter().find(|s| same_path(&s.path, &d.mount));
            let mount = d.mount.clone();
            let mut card = div()
                .w(px(330.))
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(palette.line)
                .bg(palette.surface)
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
                        .bg(palette.line)
                        .rounded_sm()
                        .child(
                            div()
                                .h_full()
                                .w(relative(if d.total > 0 {
                                    d.total.saturating_sub(d.free) as f32 / d.total as f32
                                } else {
                                    0.
                                }))
                                .bg(palette.accent),
                        ),
                )
                .child(format!(
                    "{} used of {}",
                    format_size(d.total.saturating_sub(d.free) as f64),
                    format_size(d.total as f64)
                ))
                .child(
                    div()
                        .text_color(palette.muted)
                        .child(format!("{} free", format_size(d.free as f64))),
                );
            let mut actions = div().flex().gap_2().justify_end();
            if let Some(saved) = saved {
                let (old, delta) = crate::format::staleness(saved, Some(d), crate::format::now());
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(if old || delta.is_some() {
                            palette.warning
                        } else {
                            palette.muted
                        })
                        .child(format!(
                            "Last scanned {} · {}",
                            age(saved.scanned_at),
                            format_size(saved.size as f64)
                        )),
                );
                let saved = saved.clone();
                actions = actions.child(self.button(
                    ("open-drive", index),
                    "Open last scan",
                    true,
                    move |this, cx| this.open(saved.clone(), cx),
                    cx,
                ));
            }
            actions = actions.child(self.button(
                ("scan-drive", index),
                "Scan drive",
                true,
                move |this, cx| this.start(mount.clone(), cx),
                cx,
            ));
            drives = drives.child(card.child(actions));
        }
        body = body.child(section.child(drives));
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
                    .bg(palette.surface)
                    .rounded_md()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(middle_path(&saved.path, 90))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(
                                        if crate::format::staleness(
                                            &saved,
                                            None,
                                            crate::format::now(),
                                        )
                                        .0
                                        {
                                            palette.warning
                                        } else {
                                            palette.muted
                                        },
                                    )
                                    .child(format!(
                                        "{} · {}",
                                        age(saved.scanned_at),
                                        format_size(saved.size as f64)
                                    )),
                            ),
                    )
                    .child(self.button(
                        ("open-folder", index),
                        "Open",
                        true,
                        move |this, cx| this.open(saved.clone(), cx),
                        cx,
                    )),
            );
        }
        if !self.saved.is_empty() {
            body = body.child(
                div().flex().child(
                    self.button(
                        "clear-saved",
                        "Clear saved scans",
                        !self.saved.is_empty(),
                        |this, cx| {
                            this.work(
                                |b| b.clear_saved_scans(),
                                |this, result, cx| match result {
                                    Ok(()) => this.refresh(None, cx),
                                    Err(e) => this.error = e,
                                },
                                cx,
                            );
                        },
                        cx,
                    )
                    .px_0()
                    .py_2()
                    .mt(px(6.))
                    .border_0()
                    .bg(transparent_black())
                    .text_color(palette.muted)
                    .text_size(px(12.))
                    .hover(|s| s.bg(transparent_black())),
                ),
            );
        }
        body.into_any_element()
    }

    fn scanning(&self, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
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
                div().w(px(440.)).h(px(4.)).bg(palette.line).child(
                    div()
                        .w(px(140.))
                        .h_full()
                        .bg(palette.accent)
                        .with_animation(
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
                !self.cancelling,
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
}

impl Render for DiskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::theme::palette(cx);
        self.drain(cx);
        if self
            .menu
            .as_ref()
            .is_some_and(|(_, _, size)| *size != window.viewport_size())
        {
            self.menu = None;
        }
        self.dialog_focus(window, cx);
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
        let content = match self.screen {
            Screen::Home => self.home(cx),
            Screen::Scan => self.scanning(cx),
            Screen::Results => self.results(window, cx),
        };
        let overlays = self.overlays(window, cx);
        div()
            .relative()
            .size_full()
            .when(
                matches!(self.screen, Screen::Results)
                    && f32::from(window.viewport_size().width) > 899.,
                |d| d.min_h(px(600.)),
            )
            .flex()
            .flex_col()
            .bg(palette.bg)
            .text_color(palette.text)
            .font_family(crate::FONT_FAMILY)
            .text_sm()
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    if !this.modal() {
                        this.focus.focus(window);
                    }
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.modal() {
                    return;
                }
                let key = &event.keystroke.key;
                if key == "escape" && (this.settings_open || this.menu.is_some()) {
                    this.settings_open = false;
                    this.menu = None;
                    cx.notify();
                    return;
                }
                if key == "escape" {
                    this.search_input.update(cx, |input, cx| input.set("", cx));
                    this.pointer_tile = None;
                    this.filter = None;
                    this.set_hover(None, cx);
                    cx.notify();
                } else if matches!(this.screen, Screen::Results) {
                    if event.keystroke.modifiers.secondary() && key == "f" {
                        this.search_input
                            .update(cx, |input, cx| input.focus_select(window, cx));
                        cx.stop_propagation();
                    } else if !this.search_input.focus_handle(cx).is_focused(window)
                        && !this.files_input.focus_handle(cx).is_focused(window)
                    {
                        if key == "backspace" || (event.keystroke.modifiers.alt && key == "left") {
                            this.back(cx);
                            cx.notify();
                        }
                        if event.keystroke.modifiers.alt && key == "up" {
                            this.up(cx);
                            cx.notify();
                        }
                    }
                }
            }))
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px(px(22.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(palette.line)
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Sprawl"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .when(self.admin.supported, |d| {
                                d.child(self.button(
                                    "settings",
                                    "\u{2699}",
                                    !self.admin_busy,
                                    |this, _| this.settings_open = !this.settings_open,
                                    cx,
                                ))
                            })
                            .when(matches!(self.screen, Screen::Results), |d| {
                                d.child(self.button(
                                    "rescan",
                                    "Rescan",
                                    true,
                                    |this, cx| this.start(this.scan_path.clone(), cx),
                                    cx,
                                ))
                            })
                            .when(matches!(self.screen, Screen::Results), |d| {
                                d.child(self.button(
                                    "home",
                                    "Home",
                                    !matches!(self.screen, Screen::Scan),
                                    |this, cx| {
                                        if !this.modal()
                                            && !this.busy
                                            && !matches!(this.screen, Screen::Scan)
                                        {
                                            this.session += 1;
                                            this.menu = None;
                                            this.settings_open = false;
                                            this.requests.next();
                                            this.panels.invalidate();
                                            this.busy = false;
                                            this.screen = Screen::Home;
                                            this.hover = None;
                                            this.refresh(None, cx);
                                        }
                                    },
                                    cx,
                                ))
                            }),
                    ),
            )
            .when(!self.error.is_empty(), |d| {
                d.child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(palette.raised)
                        .flex()
                        .justify_between()
                        .child(self.error.clone())
                        .child(
                            div()
                                .id("dismiss-error")
                                .cursor_pointer()
                                .child("\u{d7}")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.error.clear();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(content)
            .children(overlays)
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
