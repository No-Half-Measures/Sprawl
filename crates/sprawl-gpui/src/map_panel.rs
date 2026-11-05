use super::*;
use crate::{layout::hit, treemap};

impl DiskApp {
    pub(super) fn results(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let mut surface = div()
            .id("treemap")
            .relative()
            .flex_1()
            .min_h_0()
            .bg(rgb(0x131c26))
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.tree.as_ref().is_none_or(|tree| tree.size == 0) {
                    return;
                }
                let cache = this.cache.borrow();
                let position = event.position - cache.bounds.origin;
                let tip_moved = tip_position(position, cache.bounds.size, this.allocated)
                    != tip_position(this.pointer, cache.bounds.size, this.allocated);
                let hover = hit(
                    &cache.tiles,
                    f32::from(position.x) as f64,
                    f32::from(position.y) as f64,
                );
                drop(cache);
                this.pointer = position;
                let id = hover.and_then(|i| this.cache.borrow().tiles[i].node.id);
                let moved = this.pointer_tile != hover;
                this.pointer_tile = hover;
                this.set_hover(id, cx);
                if moved || (hover.is_some() && tip_moved) {
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    if this.pointer_tile.take().is_some() {
                        cx.notify();
                    }
                    this.set_hover(None, cx);
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
                    this.hover = hover.and_then(|i| this.cache.borrow().tiles[i].node.id);
                    this.enter(cx);
                }
            }));
        if let Some(tree) = &self.tree
            && tree.size > 0
        {
            surface = surface.child(treemap::map(
                tree.clone(),
                self.version,
                self.cache.clone(),
                treemap::MapStyle {
                    hover: self.hover,
                    filter: self.filter.clone(),
                    matches: self.panels.matches.clone(),
                    age: self.modified && self.prefs.color_by == ColorBy::Age,
                },
                self.trace,
                self.startup,
            ));
        } else {
            surface = surface.child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(rgb(0xa5b4c5))
                    .child(if self.busy {
                        "Opening folder…"
                    } else if !self.error.is_empty() {
                        "Folder unavailable"
                    } else if self.trail.last().is_some_and(|n| n.files + n.folders == 0) {
                        "This folder is empty"
                    } else {
                        "No file data to map"
                    })
                    .child(if self.busy {
                        "Preparing the map and file details."
                    } else if !self.error.is_empty() {
                        "Rescan the folder to try again."
                    } else {
                        "Zero-byte files and empty folders appear in Contents."
                    }),
            );
        }
        if let Some(tile) = self
            .pointer_tile
            .and_then(|i| self.cache.borrow().tiles.get(i).cloned())
        {
            let cache = self.cache.borrow();
            let position = tip_position(self.pointer, cache.bounds.size, self.allocated);
            let mut tip = div()
                .absolute()
                .left(position.x)
                .top(position.y)
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
                .child(
                    div()
                        .min_w_0()
                        .w_full()
                        .truncate()
                        .child(tile.node.name.clone()),
                )
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
        self.results_layout(surface.into_any_element(), window.viewport_size(), cx)
    }
}

fn tip_position(pointer: Point<Pixels>, bounds: Size<Pixels>, allocated: bool) -> Point<Pixels> {
    point(
        px((f32::from(pointer.x) + 14.)
            .min(f32::from(bounds.width) - 265.)
            .max(4.)),
        px((f32::from(pointer.y) + 14.)
            .min(f32::from(bounds.height) - if allocated { 125. } else { 105. })
            .max(4.)),
    )
}
