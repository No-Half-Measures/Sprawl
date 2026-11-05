use super::*;
use crate::treemap;
use crate::{
    format::{
        AGE_BUCKETS, format_age, format_delta, format_scan_date, not_found_by_scan, now, staleness,
    },
    preferences::clamp_split,
};

impl DiskApp {
    fn drive(&self) -> Option<&Drive> {
        self.drives
            .iter()
            .filter(|d| {
                crate::format::is_drive_root(&self.scan_path, &d.mount)
                    || self
                        .scan_path
                        .to_lowercase()
                        .starts_with(&d.mount.to_lowercase())
            })
            .max_by_key(|d| d.mount.len())
    }
    pub(super) fn capacity(&self) -> (u64, u64) {
        let d = self.drive();
        (
            self.snapshot
                .as_ref()
                .and_then(|s| s.drive_total)
                .or(d.map(|d| d.total))
                .unwrap_or(0),
            self.snapshot
                .as_ref()
                .and_then(|s| s.drive_free)
                .or(d.map(|d| d.free))
                .unwrap_or(0),
        )
    }
    pub(super) fn not_found(&self) -> Option<u64> {
        self.drive()?;
        let current = self.trail.last()?;
        let (total, free) = self.capacity();
        let moved = self
            .snapshot
            .as_ref()
            .map_or(0, |s| s.recycled.saturating_add(s.deleted));
        not_found_by_scan(
            total,
            free,
            (if self.allocated {
                current.allocated
            } else {
                current.size
            })
            .saturating_add(moved),
        )
    }
    fn nav_button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_shrink_0()
            .whitespace_nowrap()
            .px(px(10.))
            .py(px(5.))
            .rounded_md()
            .border_1()
            .border_color(rgb(0x364454))
            .text_size(px(13.))
            .when(!enabled, |d| d.opacity(0.4))
            .when(enabled, |d| {
                d.cursor_pointer().hover(|d| d.bg(rgb(0x304156)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    action(this, cx);
                    cx.notify();
                }
            }))
            .into_any_element()
    }
    fn results_header(&self, narrow: bool, cx: &Context<Self>) -> AnyElement {
        let mut crumbs = div()
            .id("breadcrumbs")
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .overflow_x_scroll()
            .whitespace_nowrap();
        for (i, n) in self.trail.iter().enumerate() {
            if i > 0 {
                crumbs = crumbs.child(div().flex_shrink_0().text_color(rgb(0xa5b4c5)).child("›"));
            }
            crumbs = crumbs.child(
                div()
                    .id(("crumb", i))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .max_w(px(220.))
                    .truncate()
                    .cursor_pointer()
                    .child(n.name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if i + 1 < this.trail.len() {
                            this.navigate(this.trail[..=i].to_vec(), cx);
                            cx.notify();
                        }
                    })),
            );
        }
        let mut header = div()
            .w_full()
            .min_w_0()
            .px(px(25.))
            .pt_2()
            .pb(px(10.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .child(self.nav_button(
                        "back",
                        "← Back",
                        !self.history.is_empty(),
                        |this, cx| this.back(cx),
                        cx,
                    ))
                    .child(self.nav_button(
                        "up",
                        "↑ Up one folder",
                        self.trail.len() > 1,
                        |this, cx| this.up(cx),
                        cx,
                    ))
                    .child(crumbs)
                    .child(
                        div()
                            .w(px(if narrow { 180. } else { 270. }))
                            .flex_shrink_0()
                            .child(self.search_input.clone()),
                    ),
            );
        if !self.panels.search_error.is_empty() {
            header = header.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0xf4b3b3))
                    .child(self.panels.search_error.clone()),
            );
        }
        if let Some(n) = self.trail.last() {
            let mut summary = vec![format!(
                "{}{}",
                if self.drive().is_some() {
                    "Scanned "
                } else {
                    ""
                },
                format_size(n.size as f64)
            )];
            if self.allocated {
                summary.push(format!("{} on disk", format_size(n.allocated as f64)));
            }
            if let Some(saved) = &self.snapshot
                && self.drive().is_some()
                && saved.recycled > 0
            {
                summary.push(format!(
                    "{} in Recycle Bin",
                    format_size(saved.recycled as f64)
                ));
            }
            summary.push(count(n.files, "file"));
            summary.push(count(n.folders, "folder"));
            if let Some(c) = &self.panels.changes {
                summary.push(format!(
                    "{} since {}",
                    c.previous_size
                        .map_or("New".into(), |previous| if previous == n.size {
                            "No change".into()
                        } else {
                            format_delta(n.size as f64 - previous as f64)
                        }),
                    format_scan_date(c.since)
                ));
            }
            if let Some(d) = self.drive() {
                let (total, free) = self.capacity();
                summary.push(format!(
                    "{}   Used {}   Free {}   Total {}",
                    d.mount,
                    format_size(total.saturating_sub(free) as f64),
                    format_size(free as f64),
                    format_size(total as f64)
                ));
                if let Some(missing) = self.not_found() {
                    summary.push(format!("Not found {}", format_size(missing as f64)));
                }
            } else if self.trail.len() > 1 {
                let parent = &self.trail[self.trail.len() - 2];
                summary.push(format!(
                    "{} of {}",
                    percent(n.size as f64, parent.size as f64),
                    parent.name
                ));
            }
            header =
                header.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap_x_5()
                        .gap_y_2()
                        .min_w_0()
                        .child(
                            div()
                                .min_w_0()
                                .max_w_full()
                                .truncate()
                                .text_size(px(22.))
                                .child(n.name.clone()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .max_w_full()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(18.))
                                .gap_y_2()
                                .text_size(px(13.))
                                .text_color(rgb(0xa5b4c5))
                                .children(summary.into_iter().map(|text| {
                                    div().min_w_0().max_w_full().truncate().child(text)
                                })),
                        ),
                );
        }
        header.into_any_element()
    }
    fn type_panel(&self, compact: bool, cx: &Context<Self>) -> AnyElement {
        let total = self.trail.last().map_or(0., |n| n.size as f64);
        let mut bar = div()
            .h(px(8.))
            .flex_shrink_0()
            .flex()
            .gap(px(2.))
            .rounded_sm()
            .overflow_hidden()
            .bg(rgb(0x364454));
        let mut legend = div()
            .mt(px(10.))
            .flex()
            .flex_wrap()
            .gap_x(px(18.))
            .gap_y(px(6.))
            .text_size(px(12.));
        for (i, b) in self
            .panels
            .breakdown
            .iter()
            .filter(|b| b.bytes > 0)
            .enumerate()
        {
            let category = b.category.clone();
            let category2 = category.clone();
            let dim = self.filter.as_ref().is_some_and(|f| f != &b.category);
            let ink = treemap::color(&treemap::category_color(Some(&b.category)));
            bar = bar.child(
                div()
                    .id(("type-bar", i))
                    .h_full()
                    .w(relative(if total > 0. {
                        b.bytes as f32 / total as f32
                    } else {
                        0.
                    }))
                    .min_w(px(1.))
                    .bg(ink)
                    .when(dim, |d| d.opacity(0.25))
                    .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                        this.set_filter(if *h { Some(category.clone()) } else { None }, cx)
                    })),
            );
            legend = legend.child(
                div()
                    .id(("type-label", i))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .py(px(3.))
                    .cursor_pointer()
                    .when(dim, |d| d.opacity(0.4))
                    .child(div().size(px(10.)).rounded_sm().bg(ink))
                    .child(panels::category_label(&b.category))
                    .child(div().text_color(rgb(0xa5b4c5)).child(format!(
                        "{} · {}",
                        format_size(b.bytes as f64),
                        percent(b.bytes as f64, total)
                    )))
                    .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                        this.set_filter(if *h { Some(category2.clone()) } else { None }, cx)
                    })),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .px(px(25.))
            .py(px(10.))
            .when(compact, |d| d.px(px(15.)).py(px(12.)))
            .flex_shrink_0()
            .child(bar)
            .child(legend)
            .into_any_element()
    }
    fn set_filter(&mut self, filter: Option<String>, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            cx.notify();
        }
    }
    pub(super) fn results_layout(
        &self,
        surface: AnyElement,
        viewport: Size<Pixels>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let narrow = f32::from(viewport.width) <= 899.;
        let mut body = div()
            .id("results-scroll")
            .flex_1()
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(narrow, |d| {
                d.overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        // Scrolling moves the canvas under a stationary pointer.
                        this.pointer_tile = None;
                        this.set_hover(None, cx);
                        cx.notify();
                    }))
            });
        if let Some(saved) = &self.snapshot {
            let (old, delta) = staleness(saved, self.drive(), now());
            let mut text = format!(
                "Snapshot from {} · {}",
                format_scan_date(saved.scanned_at),
                format_age(saved.scanned_at, now())
            );
            if old {
                text.push_str(" · This may be out of date.");
            }
            if let Some(delta) = delta {
                text.push_str(&format!(
                    " · Free space has changed by {} since this scan.",
                    format_size(delta.abs() as f64)
                ));
            }
            body = body.child(
                div()
                    .px(px(28.))
                    .py(px(9.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_5()
                    .flex_shrink_0()
                    .bg(rgb(0x293747))
                    .text_size(px(13.))
                    .when(old || delta.is_some(), |d| d.text_color(rgb(0xf2c97d)))
                    .child(text)
                    .child(self.nav_button(
                        "snapshot-rescan",
                        "Rescan",
                        !self.busy,
                        |this, cx| this.start(this.scan_path.clone(), cx),
                        cx,
                    )),
            );
        }
        body = body.child(self.results_header(narrow, cx));
        if self.skipped > 0 {
            body = body.child(
                div()
                    .mx(px(25.))
                    .flex_shrink_0()
                    .mb_3()
                    .text_size(px(13.))
                    .text_color(rgb(0xa5b4c5))
                    .child(format!(
                        "{} couldn't be read. Run as administrator to include them.",
                        count(self.skipped, "item")
                    )),
            );
        }
        if !self.error.is_empty() {
            body = body.child(
                div()
                    .px_5()
                    .flex_shrink_0()
                    .text_color(rgb(0xf4b3b3))
                    .child(self.error.clone()),
            );
        }
        let age_mode = self.modified && self.prefs.color_by == ColorBy::Age;
        let mut map_heading = div()
            .w_full()
            .min_w_0()
            // Heading content must never consume the map's remaining height.
            .h(px(43.))
            .overflow_hidden()
            .py(px(6.))
            .flex()
            .items_center()
            .gap_3()
            .flex_shrink_0()
            .child(div().flex_shrink_0().whitespace_nowrap().child("Space map"));
        if self.modified {
            let mut toggle = div().flex().flex_shrink_0().items_center();
            for (i, (mode, label)) in [(ColorBy::Type, "Type"), (ColorBy::Age, "Age")]
                .into_iter()
                .enumerate()
            {
                toggle = toggle.child(
                    div()
                        .id(("color", i))
                        .flex_shrink_0()
                        .px_2()
                        .py_1()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when((mode == ColorBy::Age) == age_mode, |d| d.bg(rgb(0x293747)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.prefs.color_by = mode;
                            this.save_prefs(cx);
                            cx.notify();
                        })),
                );
            }
            map_heading = map_heading.child(toggle);
        }
        if age_mode {
            let mut legend = div()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .text_size(px(11.))
                .text_color(rgb(0xa5b4c5));
            for (label, color) in AGE_BUCKETS {
                legend = legend.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().size(px(8.)).flex_shrink_0().bg(treemap::color(color)))
                        .child(div().min_w_0().truncate().child(label)),
                );
            }
            map_heading = map_heading.child(legend);
        } else {
            map_heading = map_heading.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .text_size(px(12.))
                    .text_color(rgb(0xa5b4c5))
                    .child(if self.busy {
                        "Loading folder…"
                    } else {
                        "Bigger blocks use more space · Click a folder to zoom in"
                    }),
            );
        }
        body.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .border_y_1()
                .border_color(rgb(0x364454))
                // GPUI's flex_none does not reset flex_1's zero basis.
                .when(narrow, |d| {
                    d.flex_none().flex_basis(Length::Auto).flex_col()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .pl(px(25.))
                        .pr(px(15.))
                        .pb(px(15.))
                        .flex()
                        .flex_col()
                        .child(map_heading)
                        .when(narrow, |d| {
                            d.flex_none()
                                .flex_basis(Length::Auto)
                                .h(px((f32::from(viewport.height) * 0.55).clamp(340., 520.)))
                                .pl(px(if f32::from(viewport.width) <= 760. {
                                    15.
                                } else {
                                    20.
                                }))
                                .pr(px(20.))
                                .pb(px(16.))
                        })
                        .child(surface),
                )
                .child(self.details(narrow, cx)),
        )
        .child(self.type_panel(f32::from(viewport.width) <= 760., cx))
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if this.dragging && !narrow {
                let bounds = *this.details_bounds.borrow();
                let height = f32::from(bounds.size.height);
                if height > 0. {
                    let split =
                        clamp_split(f32::from(event.position.y - bounds.origin.y) / height * 100.);
                    if this.prefs.split != split {
                        this.prefs.split = split;
                        cx.notify();
                    }
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.dragging {
                    this.dragging = false;
                    this.save_prefs(cx);
                    cx.notify();
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.dragging {
                    this.dragging = false;
                    this.save_prefs(cx);
                    cx.notify();
                }
            }),
        )
        .into_any_element()
    }
}
