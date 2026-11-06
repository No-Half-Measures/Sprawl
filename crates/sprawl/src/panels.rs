use super::*;
use crate::treemap;
use crate::{
    format::{format_date, format_delta, format_scan_date},
    state::linked_hover,
};
use sprawl_core::{
    changes::{Changes, GrownFile},
    scanner::{Breakdown, ExtensionTotal, LargeFile},
};
use std::collections::HashSet;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Largest,
    All,
    Extensions,
    Grown,
}
#[derive(Clone)]
pub struct FileRow {
    id: usize,
    name: String,
    path: String,
    size: u64,
    modified: String,
    folder: bool,
    delta: Option<String>,
    previous: Option<String>,
}
impl From<LargeFile> for FileRow {
    fn from(f: LargeFile) -> Self {
        Self {
            id: f.id,
            name: f.name,
            path: f.relative_path,
            size: f.size,
            modified: format_date(f.modified),
            folder: false,
            delta: None,
            previous: None,
        }
    }
}
impl FileRow {
    pub fn grown(f: &GrownFile) -> Self {
        Self {
            id: f.id,
            name: f.name.clone(),
            path: f.relative_path.clone(),
            size: f.size,
            modified: format_date(f.modified),
            folder: false,
            delta: Some(format_delta(
                f.size as f64 - f.previous_size.unwrap_or(0) as f64,
            )),
            previous: Some(
                f.previous_size
                    .map_or("New".into(), |n| format!("was {}", format_size(n as f64))),
            ),
        }
    }
}
#[derive(Default)]
pub struct Panels {
    pub largest: Vec<FileRow>,
    pub grown: Vec<FileRow>,
    pub breakdown: Vec<Breakdown>,
    pub changes: Option<Changes>,
    extensions: Option<Vec<ExtensionTotal>>,
    files: Vec<FileRow>,
    total: Option<usize>,
    files_busy: bool,
    files_error: String,
    search: Vec<FileRow>,
    search_total: Option<u64>,
    pub search_error: String,
    pub matches: Option<Rc<HashSet<usize>>>,
    pub query: String,
    file_query: String,
    tab: Tab,
    sort: String,
    descending: bool,
    search_tag: RequestTag,
    file_tag: RequestTag,
    extension_tag: RequestTag,
    contents_scroll: UniformListScrollHandle,
    largest_scroll: UniformListScrollHandle,
    all_scroll: UniformListScrollHandle,
    extension_scroll: UniformListScrollHandle,
    search_scroll: UniformListScrollHandle,
    grown_scroll: UniformListScrollHandle,
}
impl Panels {
    pub fn new() -> Self {
        Self {
            descending: true,
            ..Self::default()
        }
    }
    pub fn invalidate(&mut self) {
        self.search_tag.next();
        self.file_tag.next();
        self.extension_tag.next();
    }
    pub fn reset(&mut self) {
        self.invalidate();
        self.largest.clear();
        self.grown.clear();
        self.breakdown.clear();
        self.changes = None;
        self.extensions = None;
        self.files.clear();
        self.total = None;
        self.search.clear();
        self.search_total = None;
        self.matches = None;
        self.files_error.clear();
        self.search_error.clear();
        self.files_busy = false;
        for s in [
            &self.contents_scroll,
            &self.largest_scroll,
            &self.all_scroll,
            &self.extension_scroll,
            &self.search_scroll,
            &self.grown_scroll,
        ] {
            s.scroll_to_item(0, ScrollStrategy::Top);
        }
    }
}
fn heading(
    palette: &crate::theme::Palette,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
) -> Div {
    div()
        .h(px(43.))
        .flex_shrink_0()
        .px_4()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(div().flex_shrink_0().child(title.into()))
        .child(
            div()
                .min_w_0()
                .text_size(px(12.))
                .text_color(palette.muted)
                .truncate()
                .child(detail.into()),
        )
}
fn quiet(palette: &crate::theme::Palette, text: impl Into<SharedString>) -> AnyElement {
    div()
        .p_4()
        .text_size(px(13.))
        .text_color(palette.muted)
        .child(text.into())
        .into_any_element()
}
impl DiskApp {
    pub(super) fn set_hover(&mut self, id: Option<usize>, cx: &mut Context<Self>) {
        if self.hover == id {
            return;
        }
        self.hover = id;
        self.linked_hover = self.tree.as_ref().and_then(|t| linked_hover(t, id));
        cx.notify();
    }
    fn open_id(&mut self, id: usize, cx: &mut Context<Self>) {
        let tag = self.requests.next();
        self.work(
            move |b| b.get_trail(id),
            move |this, result, cx| {
                if !this.requests.accepts(tag) {
                    return;
                }
                match result {
                    Ok(trail) => this.navigate(trail.into_iter().map(Node::from).collect(), cx),
                    Err(e) => this.error = e,
                }
            },
            cx,
        );
    }
    pub(super) fn search_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).text().trim().to_owned();
        self.panels.query = query.clone();
        let tag = self.panels.search_tag.next();
        self.panels.search.clear();
        self.panels.search_total = None;
        self.panels.search_error.clear();
        self.panels.matches = None;
        self.panels
            .search_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
        if query.is_empty() || !matches!(self.screen, Screen::Results) {
            return;
        }
        let Some(id) = self.trail.last().and_then(|n| n.id) else {
            return;
        };
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = weak.update(cx, |this, cx| {
                if !this.panels.search_tag.accepts(tag) {
                    return;
                }
                this.work(
                    move |b| b.search(id, query, 200),
                    move |this, result, _| {
                        if !this.panels.search_tag.accepts(tag) {
                            return;
                        }
                        match result {
                            Ok(found) => {
                                this.panels.search_total = Some(found.total);
                                this.panels.matches =
                                    Some(Rc::new(found.results.iter().map(|m| m.id).collect()));
                                this.panels.search = found
                                    .results
                                    .into_iter()
                                    .map(|m| FileRow {
                                        id: m.id,
                                        name: m.name,
                                        path: m.relative_path,
                                        size: m.size,
                                        modified: format_date(m.modified),
                                        folder: m.kind == Kind::Dir,
                                        delta: None,
                                        previous: None,
                                    })
                                    .collect();
                            }
                            Err(e) => this.panels.search_error = e,
                        }
                    },
                    cx,
                );
            });
        })
        .detach();
    }
    pub(super) fn files_changed(&mut self, cx: &mut Context<Self>) {
        self.panels.file_query = self.files_input.read(cx).text().trim().to_owned();
        let tag = self.panels.file_tag.next();
        self.panels.files.clear();
        self.panels.total = None;
        self.panels.files_error.clear();
        self.panels
            .all_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
        if self.panels.tab != Tab::All || !matches!(self.screen, Screen::Results) {
            return;
        }
        self.panels.files_busy = true;
        let delay = if self.panels.file_query.is_empty() {
            0
        } else {
            200
        };
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = weak.update(cx, |this, cx| {
                if this.panels.file_tag.accepts(tag) {
                    this.load_files(0, tag, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn load_files(&mut self, offset: usize, tag: u64, cx: &mut Context<Self>) {
        let Some(id) = self.trail.last().and_then(|n| n.id) else {
            return;
        };
        let query = self.panels.file_query.clone();
        let sort = if self.panels.sort.is_empty() {
            "size".into()
        } else {
            self.panels.sort.clone()
        };
        let descending = self.panels.descending;
        self.panels.files_busy = true;
        self.panels.files_error.clear();
        self.work(
            move |b| b.get_files(id, query, sort, descending, offset, 200),
            move |this, result, _| {
                if !this.panels.file_tag.accepts(tag) {
                    return;
                }
                this.panels.files_busy = false;
                match result {
                    Ok(page) => {
                        this.panels.total = Some(page.total);
                        if offset == 0 {
                            this.panels.files.clear();
                        }
                        this.panels
                            .files
                            .extend(page.files.into_iter().map(FileRow::from));
                    }
                    Err(e) => this.panels.files_error = e,
                }
            },
            cx,
        );
    }
    pub(super) fn load_tab(&mut self, cx: &mut Context<Self>) {
        match self.panels.tab {
            Tab::All => self.files_changed(cx),
            Tab::Extensions => {
                if self.panels.extensions.is_some() {
                    return;
                }
                let Some(id) = self.trail.last().and_then(|n| n.id) else {
                    return;
                };
                let tag = self.panels.extension_tag.next();
                self.work(
                    move |b| b.get_extension_breakdown(id),
                    move |this, result, _| {
                        if !this.panels.extension_tag.accepts(tag) {
                            return;
                        }
                        match result {
                            Ok(rows) => this.panels.extensions = Some(rows),
                            Err(e) => this.panels.files_error = e,
                        }
                    },
                    cx,
                );
            }
            _ => {}
        }
    }
    fn change_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.panels.tab == tab {
            return;
        }
        self.panels.invalidate();
        self.panels.tab = tab;
        self.panels.files_error.clear();
        self.load_tab(cx);
        cx.notify();
    }
    fn file_row(&self, row: &FileRow, index: usize, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let id = row.id;
        let folder = row.folder;
        div()
            .id(("file-row", index))
            // Uniform-list items are roots: fill the panel before distributing columns.
            .w_full()
            .min_w_0()
            .h(px(61.))
            .px_4()
            .py_2()
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(13.))
            .when(self.linked_hover == Some(id), |d| d.bg(palette.hover))
            .hover(|d| d.bg(palette.hover))
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hover(hovered.then_some(id), cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.context_id(id, e.position, window, cx)
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if folder {
                    this.open_id(id, cx);
                } else {
                    this.reveal_id(id, cx);
                }
            }))
            .child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child((index + 1).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    // GPUI keeps the first nowrap measurement; truncate in a flex row with flex_1.
                    .child(div().w_full().min_w_0().overflow_hidden().flex().child(
                        div().flex_1().min_w_0().truncate().child(format!(
                            "{}{}",
                            if folder { "▱ " } else { "" },
                            row.name
                        )),
                    ))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(palette.muted)
                            .child(div().flex_1().min_w_0().truncate().child(row.path.clone()))
                            .when(self.modified && !row.modified.is_empty(), |d| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .whitespace_nowrap()
                                        .child(row.modified.clone()),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .w(px(84.))
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .when(row.delta.is_some(), |d| d.text_color(palette.growth))
                            .child(
                                row.delta
                                    .clone()
                                    .unwrap_or_else(|| format_size(row.size as f64)),
                            ),
                    )
                    .child(div().text_size(px(12.)).text_color(palette.muted).child(
                        row.previous.clone().unwrap_or_else(|| {
                            percent(
                                row.size as f64,
                                self.trail.last().map_or(0., |n| n.size as f64),
                            )
                        }),
                    )),
            )
            .into_any_element()
    }
    fn contents(&self, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let count = self
            .tree
            .as_ref()
            .and_then(|n| n.children.as_ref())
            .map_or(0, Vec::len);
        let removed = self
            .panels
            .changes
            .as_ref()
            .is_some_and(|c| c.removed.items > 0);
        let missing = self.not_found().is_some();
        let list = uniform_list(
            "contents-list",
            count + usize::from(removed) + usize::from(missing),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        if index >= count {
                            if index == count && removed {
                                let c = this.panels.changes.as_ref().unwrap();
                                return div().w_full().min_w_0().h(px(68.)).px_4().py_2().text_color(palette.shrink).child(format!(
                                    "{}    −{}",
                                    crate::format::count(c.removed.items as u64, "removed item"),
                                    format_size(c.removed.bytes as f64)
                                )).into_any_element();
                            }
                            let bytes = this.not_found().unwrap_or(0);
                            let (total, free) = this.capacity();
                            let used = total.saturating_sub(free);
                            return div().w_full().min_w_0().h(px(68.)).px_4().py_2().text_size(px(12.)).flex().flex_col().gap_1()
                                .child("Not found by scan")
                                .child(div().w_full().min_w_0().overflow_hidden().flex().child(div().flex_1().min_w_0().truncate().text_color(palette.muted).child(if this.skipped > 0 {format!("Includes {} that couldn't be read, system files and file-system overhead.", crate::format::count(this.skipped,"item"))} else {"System files, restore points and file-system overhead.".into()})))
                                .child(div().flex().justify_end().gap_2().child(format_size(bytes as f64)).child(percent(bytes as f64,used as f64)))
                                .child(div().h(px(2.)).bg(palette.line).child(div().h_full().bg(palette.muted).w(relative(if used>0 {bytes as f32/used as f32}else{0.}))))
                                .into_any_element();
                        }
                        let n = &this.tree.as_ref().unwrap().children.as_ref().unwrap()[index];
                        let id = n.id;
                        let folder = n.kind == Kind::Dir;
                        let ink = if folder {
                            palette.accent
                        } else {
                            treemap::color(&treemap::category_color(n.category.as_deref()))
                        };
                        let previous = this
                            .panels
                            .changes
                            .as_ref()
                            .and_then(|c| c.children.iter().find(|c| Some(c.id) == id))
                            .map(|c| c.previous_size);
                        let delta = previous.filter(|p| p.is_none_or(|p| p != n.size)).map(|p| {
                            p.map_or("New".into(), |p| format_delta(n.size as f64 - p as f64))
                        });
                        let total = this.trail.last().map_or(0., |n| n.size as f64);
                        div()
                            .id(("content", index))
                            .w_full()
                            .min_w_0()
                            .h(px(68.))
                            .px_4()
                            .py_2()
                            .text_size(px(13.))
                            .when(id.is_some() && this.linked_hover == id, |d| {
                                d.bg(palette.hover)
                            })
                            .hover(|d| d.bg(palette.hover))
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                                this.set_hover(if *h { id } else { None }, cx)
                            }))
                            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, window, cx| { if let Some(id) = id { this.context_id(id, e.position, window, cx); } }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if folder {
                                    this.hover = id;
                                    this.enter(cx);
                                } else if let Some(id) = id { this.reveal_id(id, cx); }
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().w(px(18.)).flex_shrink_0().text_color(ink).child(if folder {
                                        "▱"
                                    } else if id.is_none() {
                                        "⋯"
                                    } else {
                                        "▪"
                                    }))
                                    .child(
                                        div().flex_1().min_w_0().truncate().child(n.name.clone()),
                                    )
                                    .when(folder, |d| d.child(div().flex_shrink_0().child("›"))),
                            )
                            .child(
                                div()
                                    .ml(px(26.))
                                    .mt_1()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_color(palette.muted)
                                            .child(if folder {
                                                crate::format::count(n.files, "file")
                                            } else {
                                                category_label(
                                                    n.category.as_deref().unwrap_or("other"),
                                                )
                                            }),
                                    )
                                    .when_some(delta, |d, delta| {
                                        let shrink = previous.flatten().is_some_and(|p| n.size < p);
                                        d.child(div().flex_shrink_0().whitespace_nowrap().text_color(if shrink {palette.shrink} else {palette.growth}).child(delta))
                                    })
                                    .child(div().flex_shrink_0().whitespace_nowrap().child(format_size(n.size as f64)))
                                    .child(
                                        div()
                                            .w(px(58.))
                                            .flex_shrink_0()
                                            .whitespace_nowrap()
                                            .text_right()
                                            .text_color(palette.muted)
                                            .child(percent(n.size as f64, total)),
                                    ),
                            )
                            .child(
                                div()
                                    .ml(px(26.))
                                    .mt(px(7.))
                                    .h(px(2.))
                                    .bg(palette.line)
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(if total > 0. {
                                                n.size as f32 / total as f32
                                            } else {
                                                0.
                                            }))
                                            .bg(ink),
                                    ),
                            )
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .w_full()
        .min_w_0()
        .track_scroll(self.panels.contents_scroll.clone())
        .flex_1()
        .min_h_0();
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .h_full()
            .child(heading(&palette, "Contents", "Largest first"))
            .when(self.busy, |d| d.child(quiet(&palette, "Loading folder…")))
            .when(!self.busy && count == 0, |d| {
                d.child(quiet(&palette, "No items in this folder."))
            })
            .child(list)
            .into_any_element()
    }
    fn lower_panel(&self, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let tab = if self.panels.tab == Tab::Grown && self.panels.changes.is_none() {
            Tab::Largest
        } else {
            self.panels.tab
        };
        let mut tabs = div()
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_wrap()
            .gap_1()
            .px_3()
            .py_2()
            .flex_shrink_0();
        for (i, (t, label)) in [
            (Tab::Largest, "Largest files".into()),
            (Tab::All, "All files".into()),
            (Tab::Extensions, "Extensions".into()),
            (
                Tab::Grown,
                self.panels.changes.as_ref().map_or(String::new(), |c| {
                    format!("Grown since {}", format_scan_date(c.since))
                }),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if t == Tab::Grown && self.panels.changes.is_none() {
                continue;
            }
            tabs = tabs.child(
                div()
                    .id(("tab", i))
                    .min_w_0()
                    .max_w_full()
                    .truncate()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .when(t == tab, |d| d.bg(palette.raised))
                    .when(t != tab, |d| d.text_color(palette.muted))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.change_tab(t, cx))),
            );
        }
        let mut section_heading = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .pr_4()
            .flex_shrink_0()
            .child(tabs);
        if matches!(tab, Tab::Largest | Tab::Grown) {
            let count = if tab == Tab::Grown {
                self.panels.grown.len()
            } else {
                self.panels.largest.len()
            };
            section_heading = section_heading.child(
                div()
                    .min_w_0()
                    .ml_4()
                    .mb_1()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child(format!("Top {count} in this folder")),
            );
        }
        let mut panel = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .min_h_0()
            .h_full()
            .child(section_heading);
        if tab == Tab::All {
            let mut sorts = div().flex().items_center().gap_2();
            for sort in ["size", "name", "modified"] {
                if sort == "modified" && !self.modified {
                    continue;
                }
                let selected =
                    self.panels.sort == sort || (sort == "size" && self.panels.sort.is_empty());
                sorts = sorts.child(
                    div()
                        .id(sort)
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .when(selected, |d| d.bg(palette.raised))
                        .child(format!(
                            "{}{}",
                            match sort {
                                "size" => "Size",
                                "name" => "Name",
                                _ => "Modified",
                            },
                            if selected {
                                if self.panels.descending {
                                    " ↓"
                                } else {
                                    " ↑"
                                }
                            } else {
                                ""
                            }
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.panels.descending = if this.panels.sort == sort
                                || (sort == "size" && this.panels.sort.is_empty())
                            {
                                !this.panels.descending
                            } else {
                                sort == "size"
                            };
                            this.panels.sort = sort.into();
                            this.files_changed(cx);
                        })),
                );
            }
            sorts = sorts.child(div().ml_auto().text_color(palette.muted).child(
                self.panels.total.map_or(
                    if self.panels.files_busy {
                        "Loading…".into()
                    } else {
                        String::new()
                    },
                    |n| crate::format::count(n as u64, "file"),
                ),
            ));
            panel = panel.child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(12.))
                    .child(self.files_input.clone())
                    .child(sorts),
            );
        }
        if !self.panels.files_error.is_empty() && matches!(tab, Tab::All | Tab::Extensions) {
            panel = panel.child(quiet(&palette, self.panels.files_error.clone()));
        }
        if tab == Tab::Extensions {
            if let Some(rows) = &self.panels.extensions {
                if rows.is_empty() {
                    panel = panel.child(quiet(&palette, "No files found."));
                }
                panel = panel.child(
                    uniform_list(
                        "extensions-list",
                        rows.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|i| {
                                    let row = &this.panels.extensions.as_ref().unwrap()[i];
                                    let ext = row.extension.clone();
                                    let total = this.trail.last().map_or(0., |n| n.size as f64);
                                    div()
                                        .id(("ext", i))
                                        .w_full()
                                        .min_w_0()
                                        .h(px(68.))
                                        .px_4()
                                        .py_2()
                                        .cursor_pointer()
                                        .text_size(px(13.))
                                        .hover(|d| d.bg(palette.hover))
                                        .child(
                                            div()
                                                .flex()
                                                .gap_2()
                                                .items_center()
                                                .child(div().size(px(10.)).flex_shrink_0().bg(
                                                    treemap::color(&treemap::category_color(Some(
                                                        &row.category,
                                                    ))),
                                                ))
                                                .child(div().flex_1().min_w_0().truncate().child(
                                                    if ext.is_empty() {
                                                        "No extension".into()
                                                    } else {
                                                        format!(".{ext}")
                                                    },
                                                )),
                                        )
                                        .child(
                                            div()
                                                .mt_1()
                                                .flex()
                                                .justify_between()
                                                .text_size(px(12.))
                                                .text_color(palette.muted)
                                                .child(
                                                    div().flex_1().min_w_0().truncate().child(
                                                        crate::format::count(row.files, "file"),
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .whitespace_nowrap()
                                                        .child(format!(
                                                            "{}   {}",
                                                            format_size(row.bytes as f64),
                                                            percent(row.bytes as f64, total)
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div().mt(px(7.)).h(px(2.)).bg(palette.line).child(
                                                div()
                                                    .h_full()
                                                    .w(relative(if total > 0. {
                                                        row.bytes as f32 / total as f32
                                                    } else {
                                                        0.
                                                    }))
                                                    .bg(treemap::color(&treemap::category_color(
                                                        Some(&row.category),
                                                    ))),
                                            ),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !ext.is_empty() {
                                                this.search_input.update(cx, |input, cx| {
                                                    input.set(&format!("*.{ext}"), cx)
                                                });
                                            }
                                        }))
                                        .into_any_element()
                                })
                                .collect()
                        }),
                    )
                    .w_full()
                    .min_w_0()
                    .track_scroll(self.panels.extension_scroll.clone())
                    .flex_1()
                    .min_h_0(),
                );
            } else if self.panels.files_error.is_empty() {
                panel = panel.child(quiet(&palette, "Loading extensions…"));
            }
        } else {
            let (rows, scroll) = match tab {
                Tab::All => (&self.panels.files, &self.panels.all_scroll),
                Tab::Grown => (&self.panels.grown, &self.panels.grown_scroll),
                _ => (&self.panels.largest, &self.panels.largest_scroll),
            };
            let more = tab == Tab::All && self.panels.total.is_some_and(|total| rows.len() < total);
            if rows.is_empty() && !self.busy && !self.panels.files_busy {
                panel = panel.child(quiet(
                    &palette,
                    if tab == Tab::Grown {
                        "No files grew since the previous scan."
                    } else {
                        "No files found."
                    },
                ));
            }
            panel = panel.child(
                uniform_list(
                    "files-list",
                    rows.len() + usize::from(more),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| {
                                let rows = match tab {
                                    Tab::All => &this.panels.files,
                                    Tab::Grown => &this.panels.grown,
                                    _ => &this.panels.largest,
                                };
                                if let Some(row) = rows.get(i) {
                                    this.file_row(row, i, cx)
                                } else {
                                    div()
                                        .w_full()
                                        .h(px(61.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .id("show-more")
                                                .px(px(14.))
                                                .py_2()
                                                .border_1()
                                                .border_color(palette.line)
                                                .rounded_md()
                                                .bg(palette.raised)
                                                .when(this.panels.files_busy, |d| d.opacity(0.4))
                                                .when(!this.panels.files_busy, |d| {
                                                    d.cursor_pointer()
                                                        .hover(|d| d.bg(palette.hover))
                                                })
                                                .child(if this.panels.files_busy {
                                                    "Loading…"
                                                } else {
                                                    "Show more"
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if !this.panels.files_busy {
                                                        let tag = this.panels.file_tag.next();
                                                        this.load_files(
                                                            this.panels.files.len(),
                                                            tag,
                                                            cx,
                                                        );
                                                        cx.notify();
                                                    }
                                                })),
                                        )
                                        .into_any_element()
                                }
                            })
                            .collect()
                    }),
                )
                .w_full()
                .min_w_0()
                .track_scroll(scroll.clone())
                .flex_1()
                .min_h_0(),
            );
        }
        panel.into_any_element()
    }
    fn search_panel(&self, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let detail = self.panels.search_total.map_or(String::new(), |n| {
            format!(
                "{}{}",
                crate::format::count(n, "match").replace("matchs", "matches"),
                if n > self.panels.search.len() as u64 {
                    format!(", showing largest {}", self.panels.search.len())
                } else {
                    String::new()
                }
            )
        });
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(heading(&palette, "Search", detail))
            .when(
                self.panels.search_total.is_none() && self.panels.search_error.is_empty(),
                |d| d.child(quiet(&palette, "Searching…")),
            )
            .when(self.panels.search_total == Some(0), |d| {
                d.child(quiet(&palette, "No matches"))
            })
            .child(
                uniform_list(
                    "search-list",
                    self.panels.search.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| this.file_row(&this.panels.search[i], i, cx))
                            .collect()
                    }),
                )
                .w_full()
                .min_w_0()
                .track_scroll(self.panels.search_scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }
    pub(super) fn details(&self, narrow: bool, cx: &Context<Self>) -> AnyElement {
        let palette = crate::theme::palette(cx);
        let bounds = self.details_bounds.clone();
        let mut panel = div()
            .id("details-panel")
            .relative()
            .w(relative(0.3))
            .min_w(px(300.))
            .max_w(px(420.))
            .flex_shrink_0()
            .min_h_0()
            .bg(palette.surface)
            .border_l_1()
            .border_color(palette.line)
            .when(narrow, |d| {
                d.w_full()
                    .min_w_0()
                    .max_w_full()
                    .h(px(361.))
                    .border_l_0()
                    .border_t_1()
                    // Contain wheel scrolling in the lists, including at either end.
                    // Child list listeners run before this bubbling listener.
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            })
            .child(
                canvas(
                    move |rect, _, _| {
                        *bounds.borrow_mut() = rect;
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if !self.panels.query.is_empty() {
            return panel.child(self.search_panel(cx)).into_any_element();
        }
        panel = panel
            .flex()
            .when(!narrow, |d| d.flex_col())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(px(0.))
                    .map(|mut d| {
                        d.style().flex_grow = Some(if narrow { 1. } else { self.prefs.split });
                        d
                    })
                    .when(narrow, |d| {
                        d.flex_none()
                            .flex_basis(Length::Auto)
                            .w(relative(0.5))
                            .h(px(360.))
                    })
                    .overflow_hidden()
                    .child(self.contents(cx)),
            )
            .when(!narrow, |d| {
                d.child(
                    div()
                        .id("divider")
                        .h(px(7.))
                        .flex_shrink_0()
                        .cursor(CursorStyle::ResizeUpDown)
                        .hover(|d| d.bg(palette.hover))
                        .child(div().mt(px(3.)).h(px(1.)).bg(if self.dragging {
                            palette.accent
                        } else {
                            palette.line
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                if event.click_count == 2 {
                                    this.dragging = false;
                                    this.prefs.split = 48.;
                                    this.save_prefs(cx);
                                } else {
                                    this.dragging = true;
                                }
                                cx.notify();
                            }),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(px(0.))
                    .map(|mut d| {
                        d.style().flex_grow =
                            Some(if narrow { 1. } else { 100. - self.prefs.split });
                        d
                    })
                    .when(narrow, |d| d.border_l_1().border_color(palette.line))
                    .when(narrow, |d| {
                        d.flex_none()
                            .flex_basis(Length::Auto)
                            .w(relative(0.5))
                            .h(px(360.))
                    })
                    .overflow_hidden()
                    .child(self.lower_panel(cx)),
            );
        panel.into_any_element()
    }
    pub(super) fn save_prefs(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.prefs.save(&self.prefs_path) {
            self.error = format!("Could not save panel preferences: {e}");
            cx.notify();
        }
    }
}
pub(super) fn category_label(id: &str) -> String {
    static DATA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let data = DATA
        .get_or_init(|| serde_json::from_str(sprawl_core::CATEGORIES_JSON).expect("categories"));
    data.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_str() == Some(id))
        .and_then(|c| c["label"].as_str())
        .unwrap_or(id)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn stale_panel_responses_and_debounce_are_rejected() {
        let mut panels = Panels::new();
        assert!(panels.descending);
        let old_query = panels.search_tag.next();
        let current_query = panels.search_tag.next();
        assert!(!panels.search_tag.accepts(old_query));
        assert!(panels.search_tag.accepts(current_query));
        let old_sort = panels.file_tag.next();
        let current_sort = panels.file_tag.next();
        assert!(!panels.file_tag.accepts(old_sort));
        let extensions = panels.extension_tag.next();
        panels.invalidate();
        assert!(!panels.search_tag.accepts(current_query));
        assert!(!panels.file_tag.accepts(current_sort));
        assert!(!panels.extension_tag.accepts(extensions));
        let old_folder = panels.file_tag.next();
        panels.reset();
        assert!(!panels.file_tag.accepts(old_folder));
    }
}
