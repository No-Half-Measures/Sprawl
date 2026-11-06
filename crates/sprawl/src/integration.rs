use super::*;
use crate::actions::{self, Command, DeleteEffect, DeleteInput, DeleteState};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const CONTEXT_MENU_WIDTH: f32 = 205.;

pub(super) struct DeleteDialog {
    node: Node,
    path: String,
    state: DeleteState,
    pointer_guard: actions::DeletePointerGuard,
    permanent: bool,
    handles: [FocusHandle; 3],
    previous: Option<FocusHandle>,
    focus_pending: bool,
    armed: Option<(DeleteState, usize)>,
}
fn owner(window: &Window) -> Result<usize, String> {
    match HasWindowHandle::window_handle(window)
        .map_err(|e| e.to_string())?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as usize),
        _ => Err("A Windows window handle is required for deletion.".into()),
    }
}
impl DiskApp {
    pub(super) fn modal(&self) -> bool {
        self.delete_pending || self.deleting.is_some()
    }
    pub(super) fn notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = text;
        self.notice_serial += 1;
        let serial = self.notice_serial;
        cx.spawn(async move |weak, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let _ = weak.update(cx, |this, cx| {
                if this.notice_serial == serial {
                    this.notice.clear();
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn scan_requested(&mut self, path: String, cx: &mut Context<Self>) {
        let state = crate::state::LaunchState {
            opening: self.opening,
            prompt_open: self.prompt_open,
            delete_dialog: self.deleting.is_some(),
            delete_running: self
                .deleting
                .as_ref()
                .is_some_and(|dialog| dialog.state == DeleteState::Running),
            delete_pending: self.delete_pending,
        };
        if let Some(path) = crate::state::route_launch(state, path, &mut self.pending_launch) {
            // start invalidates tree requests and replaces an active scan synchronously.
            self.start(path, cx);
        }
    }
    pub(super) fn resume_launch(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(path) = self.pending_launch.take() {
            self.scan_requested(path, cx);
            true
        } else {
            false
        }
    }
    pub(super) fn context(
        &mut self,
        node: Node,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal() || self.busy || !matches!(self.screen, Screen::Results) || node.id.is_none()
        {
            return;
        }
        self.settings_open = false;
        let size = window.viewport_size();
        let position = point(
            position
                .x
                .max(px(0.))
                .min((size.width - px(CONTEXT_MENU_WIDTH)).max(px(0.))),
            position
                .y
                .max(px(0.))
                .min((size.height - px(205.)).max(px(0.))),
        );
        self.menu = Some((node, position, size));
        cx.notify();
    }
    pub(super) fn action(&mut self, command: Command, node: Node, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(id) = node.id else {
            return;
        };
        if self.modal() || self.busy {
            return;
        }
        let session = self.session;
        if command == Command::Delete {
            if !self.admin.supported {
                return;
            }
            if id == 0 {
                return;
            }
            self.delete_pending = true;
            self.work(
                move |b| b.delete_target(id),
                move |this, result, cx| {
                    this.delete_pending = false;
                    if this.session != session {
                        return;
                    }
                    match result {
                        Ok((target, path)) => {
                            if target.name != node.name {
                                this.error = "This item has changed. Rescan to refresh it.".into();
                                return;
                            }
                            this.deleting = Some(DeleteDialog {
                                node: target.into(),
                                path,
                                state: DeleteState::Choose,
                                pointer_guard: actions::DeletePointerGuard::new(Instant::now()),
                                permanent: false,
                                handles: std::array::from_fn(|_| cx.focus_handle()),
                                previous: None,
                                focus_pending: true,
                                armed: None,
                            })
                        }
                        Err(e) => this.error = e,
                    }
                },
                cx,
            );
        } else {
            self.work(
                move |b| match command {
                    Command::Copy => b.get_node_path(id).map(PathBuf::from),
                    Command::Open => b.open_folder(id),
                    _ => b.reveal_in_explorer(id),
                },
                move |this, result, cx| {
                    if this.session != session {
                        return;
                    }
                    match result {
                        Ok(path) => match command {
                            Command::Copy => {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    path.to_string_lossy().into_owned(),
                                ));
                                this.notice("Path copied.".into(), cx);
                            }
                            Command::Open => cx.open_with_system(&path),
                            _ => cx.reveal_path(&path),
                        },
                        Err(e) => this.error = e,
                    }
                },
                cx,
            );
        }
    }
    pub(super) fn context_id(
        &mut self,
        id: usize,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal() {
            return;
        }
        // Panel rows omit counts; obtain the actual node before exposing actions.
        match self.backend.get_nodes(vec![id]) {
            Ok(mut nodes) => {
                if let Some(Some(node)) = nodes.pop() {
                    self.context(node.into(), position, window, cx);
                }
            }
            Err(e) => {
                self.error = e;
                cx.notify();
            }
        }
    }
    pub(super) fn reveal_id(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.modal() {
            return;
        }
        let session = self.session;
        self.work(
            move |b| b.reveal_in_explorer(id),
            move |this, result, cx| {
                if this.session == session {
                    match result {
                        Ok(path) => cx.reveal_path(&path),
                        Err(e) => this.error = e,
                    }
                }
            },
            cx,
        );
    }
    pub(super) fn dialog_focus(&mut self, window: &mut Window, cx: &Context<Self>) {
        if let Some(focus) = self.restore_focus.take() {
            focus.focus(window);
        }
        if let Some(dialog) = &mut self.deleting
            && dialog.focus_pending
        {
            if dialog.previous.is_none() {
                dialog.previous = window.focused(cx);
            }
            dialog.handles[0].focus(window);
            dialog.focus_pending = false;
        }
    }
    fn close_delete(&mut self) {
        if let Some(dialog) = self.deleting.take() {
            self.restore_focus = dialog.previous;
        }
    }
    fn delete_input(&mut self, input: DeleteInput, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.deleting else {
            return;
        };
        let before = dialog.state;
        let effect = dialog.state.transition(input);
        dialog.armed = None;
        if before != dialog.state {
            dialog.focus_pending = true;
            dialog.pointer_guard.restart(Instant::now());
        }
        match effect {
            DeleteEffect::Close => self.close_delete(),
            DeleteEffect::Submit(permanent) => {
                dialog.permanent = permanent;
                let id = dialog.node.id.expect("captured real node");
                let name = dialog.node.name.clone();
                let path = dialog.path.clone();
                let hwnd = match owner(window) {
                    Ok(hwnd) => hwnd,
                    Err(e) => {
                        self.error = e;
                        self.close_delete();
                        cx.notify();
                        return;
                    }
                };
                let session = self.session;
                self.error.clear();
                let mut ids: Vec<_> = self
                    .trail
                    .iter()
                    .chain(self.history.iter().flatten())
                    .filter_map(|n| n.id)
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                self.work_on_own_thread(
                    move |b| {
                        let deleted = b.delete_item_at(id, permanent, hwnd, &path)?;
                        let nodes = b.get_nodes(ids.clone());
                        Ok::<_, String>((deleted, ids, nodes))
                    },
                    move |this, result, cx| {
                        if this.session != session {
                            return;
                        }
                        match result {
                            Ok((deleted, ids, nodes)) => {
                                this.notice(
                                    if permanent {
                                        format!("Deleted {name} permanently.")
                                    } else {
                                        format!("Moved {name} to the Recycle Bin.")
                                    },
                                    cx,
                                );
                                this.recycled = deleted.recycled;
                                this.deleted = deleted.deleted;
                                if let Some(meta) = &mut this.snapshot {
                                    meta.size = deleted.root.size;
                                    meta.files = deleted.root.files;
                                    meta.folders = deleted.root.folders;
                                    meta.recycled = deleted.recycled;
                                    meta.deleted = deleted.deleted;
                                }
                                this.hover = None;
                                this.linked_hover = None;
                                this.pointer_tile = None;
                                this.filter = None;
                                let mut refresh_error = None;
                                match nodes {
                                    Ok(nodes) => {
                                        let (trail, history) = actions::refresh_trails(
                                            &this.trail,
                                            &this.history,
                                            &ids,
                                            nodes.into_iter().map(|n| n.map(Into::into)).collect(),
                                        );
                                        this.trail = if trail.is_empty() {
                                            vec![deleted.root.into()]
                                        } else {
                                            trail
                                        };
                                        this.history = history;
                                    }
                                    Err(e) => refresh_error = Some(e),
                                }
                                this.close_delete();
                                this.request_tree(cx);
                                if let Some(e) = refresh_error {
                                    this.error = e;
                                }
                            }
                            Err(e) => {
                                if e == "Delete cancelled." {
                                    this.notice(e, cx);
                                } else {
                                    this.error = e;
                                }
                                this.close_delete();
                            }
                        }
                    },
                    cx,
                );
            }
            DeleteEffect::None => {}
        }
        cx.notify();
    }
    pub(super) fn export(&mut self, cx: &mut Context<Self>) {
        if self.exporting || self.modal() {
            return;
        }
        let Some(node) = self.trail.last() else {
            return;
        };
        let Some(id) = node.id else {
            return;
        };
        let name = actions::export_name(&node.name);
        let session = self.session;
        self.exporting = true;
        let directory = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from(&self.scan_path));
        let prompt = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |weak, cx| {
            let result = prompt.await;
            let _ = weak.update(cx, |this, cx| {
                if this.session != session {
                    this.exporting = false;
                    return;
                }
                match result {
                    Ok(Ok(Some(path))) => {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        this.error.clear();
                        this.work(
                            move |b| {
                                let format = actions::export_format(&path).into();
                                b.export_scan(id, path, format)
                            },
                            move |this, result, cx| {
                                this.exporting = false;
                                if this.session != session {
                                    return;
                                }
                                match result {
                                    Ok(items) => this.notice(
                                        format!("Exported {} to {name}.", count(items, "item")),
                                        cx,
                                    ),
                                    Err(e) => this.error = e,
                                }
                            },
                            cx,
                        );
                    }
                    Ok(Ok(None)) => this.exporting = false,
                    Ok(Err(e)) => {
                        this.exporting = false;
                        this.error = e.to_string();
                    }
                    Err(e) => {
                        this.exporting = false;
                        this.error = e.to_string();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn restart_admin(&mut self, cx: &mut Context<Self>) {
        if self.admin_busy {
            return;
        }
        self.admin_busy = true;
        self.error.clear();
        self.work_on_own_thread(
            |b| b.restart_as_admin(),
            |this, result, cx| {
                this.admin_busy = false;
                match result {
                    Ok(()) => cx.quit(),
                    Err(e) => this.error = e,
                }
            },
            cx,
        );
    }
    fn setting(&mut self, context_menu: bool, enabled: bool, cx: &mut Context<Self>) {
        if self.admin_busy {
            return;
        }
        self.admin_busy = true;
        self.error.clear();
        self.work(
            move |b| {
                let result = if context_menu {
                    b.set_context_menu(enabled)
                } else {
                    b.set_always_elevate(enabled)
                };
                (result, b.admin_status())
            },
            |this, (result, status), _| {
                this.admin = status;
                this.admin_busy = false;
                if let Err(e) = result {
                    this.error = e;
                }
            },
            cx,
        );
    }
    pub(super) fn overlays(&self, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let palette = crate::theme::palette(cx);
        let mut overlays = vec![];
        if !self.notice.is_empty() {
            overlays.push(
                deferred(
                    div()
                        .absolute()
                        .bottom(px(22.))
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .max_w(px(540.))
                                .occlude()
                                .p_3()
                                .bg(palette.hover)
                                .rounded_md()
                                .flex()
                                .gap_4()
                                .child(self.notice.clone())
                                .child(
                                    div()
                                        .id("dismiss-notice")
                                        .cursor_pointer()
                                        .child("\u{d7}")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.notice.clear();
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        if let Some((node, position, _)) = &self.menu {
            let mut menu = div()
                .id("context-menu")
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(CONTEXT_MENU_WIDTH))
                .p(px(5.))
                .text_size(px(14.))
                .line_height(px(20.))
                .bg(palette.raised)
                .border_1()
                .border_color(palette.line)
                .rounded_md()
                .occlude()
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .p(px(8.))
                        .text_size(px(12.))
                        .text_color(palette.muted)
                        .truncate()
                        .child(node.name.clone()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }));
            let session = self.session;
            for (index, command) in actions::menu_items(node)
                .into_iter()
                .filter(|command| *command != Command::Delete || self.admin.supported)
                .enumerate()
            {
                let node = node.clone();
                let label = match command {
                    Command::Open => "Open folder",
                    Command::Reveal if cfg!(windows) => "Reveal in Explorer",
                    Command::Reveal => "Reveal in file manager",
                    Command::Copy => "Copy path",
                    Command::Delete => "Delete\u{2026}",
                };
                menu = menu.child(
                    div()
                        .id(("menu-item", index))
                        .w_full()
                        .p(px(9.))
                        .cursor_pointer()
                        .hover(|d| d.bg(palette.hover))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.session == session {
                                this.action(command, node.clone(), cx);
                            }
                        })),
                );
            }
            overlays.push(
                deferred(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Middle,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            menu.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                        ),
                )
                .into_any_element(),
            );
        }
        if self.admin.supported && self.settings_open {
            let mut panel = div()
                .absolute()
                .right_4()
                .top(px(48.))
                .w(px(420.))
                .p_4()
                .bg(palette.raised)
                .rounded_md()
                .occlude()
                .flex()
                .flex_col()
                .gap_3()
                .child("Settings");
            for (index, (context_menu, checked, label)) in [
                (
                    false,
                    self.admin.always_elevate,
                    "Always run as administrator",
                ),
                (
                    true,
                    self.admin.context_menu_enabled,
                    "Add \"Scan with Sprawl\" to Explorer's right-click menu",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                panel = panel.child(
                    div()
                        .id(("setting", index))
                        .when(self.admin_busy, |d| d.opacity(0.4))
                        .cursor_pointer()
                        .child(format!("{} {label}", if checked { "[x]" } else { "[ ]" }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.setting(context_menu, !checked, cx)
                        })),
                );
            }
            overlays.push(
                deferred(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Middle,
                            cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            }),
                        )
                        .child(
                            panel
                                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                                .on_mouse_down(MouseButton::Middle, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                        ),
                )
                .into_any_element(),
            );
        }
        if self.modal() {
            let mut backdrop = div()
                .id("delete-backdrop")
                .absolute()
                .inset_0()
                .bg(palette.backdrop)
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    let Some(dialog) = &this.deleting else {
                        return;
                    };
                    if event.is_held || dialog.state == DeleteState::Running {
                        return;
                    }
                    let key = event.keystroke.key.as_str();
                    if key == "escape" {
                        this.delete_input(DeleteInput::Cancel, window, cx);
                    } else if key == "tab" {
                        let count = if dialog.state == DeleteState::Choose {
                            3
                        } else {
                            2
                        };
                        let index = dialog
                            .handles
                            .iter()
                            .position(|h| h.is_focused(window))
                            .unwrap_or(0);
                        let index = (index
                            + if event.keystroke.modifiers.shift {
                                count - 1
                            } else {
                                1
                            })
                            % count;
                        dialog.handles[index].focus(window);
                    } else if key == "enter" || key == "space" {
                        let index = dialog
                            .handles
                            .iter()
                            .position(|h| h.is_focused(window))
                            .unwrap_or(0);
                        let input = match index {
                            0 => {
                                if dialog.state == DeleteState::Choose {
                                    DeleteInput::Cancel
                                } else {
                                    DeleteInput::Back
                                }
                            }
                            1 => DeleteInput::Permanent,
                            _ => DeleteInput::Recycle,
                        };
                        this.delete_input(input, window, cx);
                    }
                }));
            if let Some(dialog) = &self.deleting {
                let permanent = dialog.state == DeleteState::Permanent
                    || (dialog.state == DeleteState::Running && dialog.permanent);
                let running = dialog.state == DeleteState::Running;
                let state = dialog.state;
                let mut body = div()
                    .id(if permanent {
                        "delete-permanent"
                    } else {
                        "delete-choose"
                    })
                    .w(px(550.))
                    .p_6()
                    .rounded_lg()
                    .bg(palette.surface)
                    .border_1()
                    .border_color(palette.line)
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_xl().child(if permanent {
                        "Delete permanently?".into()
                    } else {
                        format!("Delete {}?", dialog.node.name)
                    }));
                if !permanent {
                    body = body
                        .child(dialog.node.name.clone())
                        .child(div().whitespace_normal().child(dialog.path.clone()))
                        .child(format!(
                            "{}{}",
                            format_size(dialog.node.size as f64),
                            if dialog.node.kind == Kind::Dir {
                                format!(
                                    " \u{b7} {} \u{b7} {}",
                                    count(dialog.node.files, "file"),
                                    count(dialog.node.folders, "folder")
                                )
                            } else {
                                String::new()
                            }
                        ));
                }
                body = body.child(if permanent { format!("{} ({}) will be deleted permanently. This can't be undone.", dialog.node.name, format_size(dialog.node.size as f64)) } else { "Moving to the Recycle Bin lets you restore it later. Deleting permanently frees the space now and can't be undone.".into() });
                let mut buttons = div().flex().items_center().justify_end().gap_2();
                let labels = if permanent {
                    vec![
                        "Back",
                        if running {
                            "Deleting\u{2026}"
                        } else {
                            "Delete permanently"
                        },
                    ]
                } else {
                    vec![
                        "Cancel",
                        "Delete permanently",
                        if running {
                            "Moving\u{2026}"
                        } else {
                            "Move to Recycle Bin"
                        },
                    ]
                };
                for (index, label) in labels.into_iter().enumerate() {
                    let primary = !permanent && index == 2;
                    let danger = permanent && index == 1;
                    let danger_outline = !permanent && index == 1;
                    buttons = buttons.child(
                        div()
                            .id((
                                if permanent {
                                    "permanent-button"
                                } else {
                                    "choose-button"
                                },
                                index,
                            ))
                            .track_focus(&dialog.handles[index])
                            .tab_index(index as isize)
                            .tab_stop(!running)
                            .relative()
                            .px(px(if primary { 21. } else { 14. }))
                            .py(px(if primary { 12. } else { 8. }))
                            .rounded_md()
                            .border_1()
                            .border_color(if danger_outline {
                                palette.danger
                            } else if primary || danger {
                                transparent_black()
                            } else {
                                palette.line
                            })
                            .bg(if primary {
                                palette.accent
                            } else if danger {
                                palette.danger
                            } else if danger_outline {
                                transparent_black()
                            } else {
                                palette.raised
                            })
                            .text_color(if primary {
                                palette.accent_text
                            } else if danger {
                                palette.danger_text
                            } else if danger_outline {
                                palette.error
                            } else {
                                palette.text
                            })
                            .when(primary, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .when(!running, |d| {
                                d.cursor_pointer().hover(|s| {
                                    s.bg(if primary {
                                        palette.accent_hover
                                    } else if danger {
                                        palette.danger_hover
                                    } else {
                                        palette.hover
                                    })
                                })
                            })
                            .when(dialog.handles[index].is_focused(window), |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(-4.))
                                        .bottom(px(-4.))
                                        .left(px(-4.))
                                        .right(px(-4.))
                                        .rounded_md()
                                        .border_2()
                                        .border_color(palette.accent),
                                )
                            })
                            .when(running, |d| d.opacity(0.4))
                            .child(label)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(dialog) = &mut this.deleting
                                        && dialog.state == state
                                        && state != DeleteState::Running
                                        && dialog.pointer_guard.accepts(Instant::now())
                                    {
                                        dialog.armed = Some((state, index));
                                        dialog.handles[index].focus(window);
                                    }
                                }),
                            )
                            .on_click(cx.listener(move |this, event, window, cx| {
                                cx.stop_propagation();
                                if !matches!(event, ClickEvent::Mouse(_)) {
                                    return;
                                }
                                if this.deleting.as_ref().is_none_or(|d| {
                                    !d.pointer_guard.accepts(Instant::now())
                                        || d.state != state
                                        || d.armed != Some((state, index))
                                }) {
                                    return;
                                }
                                let input = match index {
                                    0 => {
                                        if permanent {
                                            DeleteInput::Back
                                        } else {
                                            DeleteInput::Cancel
                                        }
                                    }
                                    1 => DeleteInput::Permanent,
                                    _ => DeleteInput::Recycle,
                                };
                                this.delete_input(input, window, cx);
                            })),
                    );
                }
                backdrop = backdrop.child(body.child(buttons));
            }
            overlays.push(deferred(backdrop).into_any_element());
        }
        overlays
    }
}
