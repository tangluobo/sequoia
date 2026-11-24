mod adb;
mod chrome;
mod frame;
mod keyboard;
mod pointer;
mod vnc;
use super::*;
use tomato_services::remote_desktop::{Input, Status};

pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    let Panel::NativeDesktop {
        connection,
        session,
        view,
    } = &mut tab.panel
    else {
        return None;
    };
    if connection.kind() == "ADB" {
        return adb::render(ui, connection, session, view, &tab.id, &mut tab.error);
    }
    let mut intent = None;
    let fullscreen = view.window.as_ref().is_some_and(|window| window.fullscreen);
    let is_vnc = connection.kind() == "VNC";
    if is_vnc {
        ui.spacing_mut().item_spacing.y = 0.;
    }
    if is_vnc
        && !std::mem::replace(&mut view.fullscreen_initialized, true)
        && connection.flag("fullscreen")
    {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }
    view.protocol_clipboard = is_vnc
        || cfg!(windows)
        || connection
            .options
            .get("mapClipboard")
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    let failed = session
        .as_ref()
        .is_some_and(|session| matches!(session.status(), Status::Closed | Status::Failed(_)));
    if let Some(session) = session.as_ref() {
        let started = frame::timing_started();
        let mut updates = session.take_rdp_snapshots();
        let size = updates
            .last()
            .map(|u| [u.width, u.height])
            .unwrap_or(view.size);
        if !is_vnc
            && !size.contains(&0)
            && frame::needs_full_snapshot(ui.ctx(), view, size)
            && updates.first().is_none_or(|update| update.region.is_some())
            && let Some(full) = session.rdp_full_snapshot()
        {
            updates = vec![full];
        }
        if !updates.is_empty() {
            frame::record_rdp_upload(
                ui.ctx(),
                updates.iter().map(|u| u.pixels.len()).sum(),
                updates.len(),
            );
        }
        let changed = !updates.is_empty();
        let taken = started.map(|_| std::time::Instant::now());
        for update in updates {
            frame::upload_snapshot(ui.ctx(), view, &tab.id, update);
        }
        frame::record_timing(ui.ctx(), started, taken, changed);
    }
    // RDP graphics already come from the retained RGBA canvas above. Calling
    // take_frame as well races the producer and drains fresh RDP damage through
    // the legacy RGB conversion path, replacing a native texture with a partial
    // fallback frame. Only VNC owns that legacy frame slot.
    if is_vnc
        && let Some(session) = session.as_ref()
        && let Some(frame) = session.take_frame()
    {
        frame::upload(ui.ctx(), view, &tab.id, frame);
    }

    let vnc_bounds = if is_vnc {
        let status = session
            .as_ref()
            .map(|s| s.status())
            .unwrap_or_else(|| tab.error.clone().map_or(Status::Closed, Status::Failed));
        if status != Status::PasswordRequired && view.vnc_password_editor.is_some() {
            view.clear_vnc_password(ui.ctx());
        }
        let name = session
            .as_ref()
            .map(|s| s.desktop_name())
            .unwrap_or_default();
        let (bounds, reconnect, disconnect) =
            vnc::chrome(ui, connection, &status, &name, view.size);
        if disconnect {
            *session = None;
            view.clear_vnc_password(ui.ctx());
            view.reset_vnc_input();
            tab.error = None;
            vnc::paint_last_frame(ui, bounds, view);
            return None;
        }
        if reconnect {
            return Some(Intent::ConnectDesktop);
        }
        if status == Status::PasswordRequired {
            vnc::paint_last_frame(ui, bounds, view);
            return match vnc::password::show(
                ui.ctx(),
                &tab.id,
                connection,
                view,
                tab.error.as_deref(),
            ) {
                Some(vnc::password::Action::Submit) => Some(Intent::SubmitVncPassword),
                Some(vnc::password::Action::Cancel) => {
                    view.clear_vnc_password(ui.ctx());
                    tab.error = None;
                    if let Some(session) = session.as_ref() {
                        let _ = session.cancel_password();
                    }
                    None
                }
                None => None,
            };
        }
        if session.is_none() || failed {
            vnc::paint_last_frame(ui, bounds, view);
            return None;
        }
        Some(bounds)
    } else {
        None
    };
    if session.is_none() || failed {
        let status = session
            .as_ref()
            .map(|session| session.status())
            .unwrap_or(Status::Closed);
        chrome::render(ui, connection, &status, view.size, view.window.is_none());
        egui::Window::new(if failed {
            "远程桌面连接失败"
        } else {
            "远程桌面认证"
        })
        .id(ui.make_persistent_id(("rdp-login", &tab.id)))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.set_max_width(460.);
            ui.label(format!(
                "{}@{}:{}",
                connection.text("username"),
                connection.text("host"),
                connection.number("port")
            ));
            if let Status::Failed(error) = &status {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("连接失败：{error}"))
                            .color(Color32::from_rgb(180, 30, 30)),
                    )
                    .wrap()
                    .selectable(true),
                );
            } else if failed {
                ui.label("远程桌面会话已关闭。");
            }
            if let Some(error) = &tab.error {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(error).color(Color32::from_rgb(180, 30, 30)),
                    )
                    .wrap()
                    .selectable(true),
                );
            }
            ui.label(if failed {
                "密码（留空使用本次会话的密码）"
            } else {
                "密码"
            });
            let password = ui.add(TextEdit::singleline(&mut view.password).password(true));
            if !view.auth_focused {
                password.request_focus();
                view.auth_focused = true;
            }
            let enter = (password.has_focus() || password.lost_focus())
                && ui.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: egui::Key::Enter,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    })
                });
            ui.horizontal(|ui| {
                if ui
                    .button(if failed { "重新连接" } else { "连接" })
                    .clicked()
                    || enter
                {
                    intent = Some(Intent::ConnectDesktop);
                }
                if ui.button("取消").clicked() {
                    intent = Some(Intent::Close);
                }
            });
        });
        return intent;
    }
    let session = session.as_mut()?;
    let status = session.status();
    let bounds = if fullscreen {
        ui.max_rect()
    } else {
        vnc_bounds.unwrap_or_else(|| {
            chrome::render(ui, connection, &status, view.size, view.window.is_none())
        })
    };
    if view.texture_id().is_none() {
        ui.painter().text(
            bounds.center(),
            egui::Align2::CENTER_CENTER,
            format!("正在连接到 {} ...", connection.text("host")),
            egui::FontId::proportional(14.0),
            Color32::from_gray(230),
        );
        return None;
    }
    let remote = egui::vec2(f32::from(view.size[0]), f32::from(view.size[1]));
    let scale = if is_vnc || fullscreen {
        (bounds.width() / remote.x)
            .min(bounds.height() / remote.y)
            .max(0.001)
    } else {
        1.0
    };
    let mut surface = ui.new_child(egui::UiBuilder::new().max_rect(bounds));
    surface.set_clip_rect(bounds.intersect(ui.clip_rect()));
    surface.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
    let (rect, response) = if is_vnc {
        vnc::surface(&mut surface, bounds, view)
    } else if fullscreen {
        let size = remote * scale;
        let rect = egui::Rect::from_center_size(bounds.center(), size);
        let response = surface.interact(
            rect,
            surface.id().with("rdp-fullscreen"),
            egui::Sense::click_and_drag(),
        );
        if let Some(texture) = view.texture_id() {
            surface.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        (rect, response)
    } else {
        egui::ScrollArea::both()
            .scroll_bar_visibility(
                if view
                    .window
                    .as_ref()
                    .is_some_and(|window| !window.scrollbars)
                {
                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden
                } else {
                    egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
                },
            )
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
            .show(&mut surface, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(remote, egui::Sense::click_and_drag());
                if let Some(texture) = view.texture_id() {
                    ui.painter().image(
                        texture,
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                (rect, response)
            })
            .inner
    };
    if response.clicked() || std::mem::take(&mut view.needs_focus) {
        response.request_focus();
    }
    // Remote pointers are native OS cursors, not painted into the delayed GPU frame.
    // Clear sticky image output when leaving the desktop or entering server-hidden mode.
    ui.ctx().set_cursor_image(None);
    if is_vnc && response.contains_pointer() && session.pointer_hidden() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    }
    if !is_vnc && response.contains_pointer() {
        let (revision, pointer) = session.pointer();
        if revision != view.pointer_revision {
            view.native_pointer = pointer.as_ref().map(|pointer| egui::CustomCursorImage {
                rgba: pointer.rgba.clone().into(),
                size: pointer.size,
                hotspot: [
                    pointer.hotspot[0].min(pointer.size[0] - 1),
                    pointer.hotspot[1].min(pointer.size[1] - 1),
                ],
            });
            view.pointer_revision = revision;
        }
        if session.pointer_hidden() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        } else {
            ui.ctx().set_cursor_image(view.native_pointer.clone());
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
    }
    let focused = response.has_focus() && ui.input(|input| input.focused) && ui.is_enabled();
    let mut outgoing = Vec::new();
    let mut send = |event| outgoing.push(event);
    let point = |pos: egui::Pos2| {
        (
            ((pos.x - rect.left()) / scale).clamp(0.0, remote.x - 1.0) as u16,
            ((pos.y - rect.top()) / scale).clamp(0.0, remote.y - 1.0) as u16,
        )
    };
    if status == Status::Ready {
        if focused {
            keyboard::modifiers(view, ui.input(|input| input.modifiers), &mut send);
            let (x, y) = view.position.unwrap_or((0, 0));
            let cursor = egui::Rect::from_min_size(
                rect.min + egui::vec2(f32::from(x), f32::from(y)) * scale,
                egui::vec2(1.0, 20.0),
            );
            ui.output_mut(|output| {
                output.ime = Some(egui::output::IMEOutput {
                    purpose: Default::default(),
                    rect,
                    cursor_rect: cursor,
                    should_interrupt_composition: false,
                })
            });
            if !view.composition.is_empty() {
                ui.painter().text(
                    cursor.min,
                    egui::Align2::LEFT_TOP,
                    &view.composition,
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );
            }
        }
        let events = ui.input(|input| input.events.clone());
        // Process positions in event order. The final hover position can already
        // be past a press/release in this frame and must not precede those events.
        let has_pointer_events = events.iter().any(|event| {
            matches!(
                event,
                egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }
            )
        });
        if !has_pointer_events
            && let Some(pos) = response.hover_pos().filter(|pos| rect.contains(*pos))
        {
            pointer::motion(view, point(pos), &mut send);
        }
        let mut typing = if is_vnc && focused {
            vnc::Typing::new(&events, !view.composition.is_empty())
        } else {
            Default::default()
        };
        for mut event in events {
            if is_vnc && focused && typing.handle(&event, view, &mut send) {
                continue;
            }
            if is_vnc
                && focused
                && let egui::Event::Paste(text) = &mut event
                && connection
                    .options
                    .get("mapClipboard")
                    .and_then(serde_json::Value::as_bool)
                    != Some(false)
                && let Err(error) = session.prepare_paste(std::mem::take(text))
            {
                tab.error = Some(error.to_string());
                continue;
            }
            if let egui::Event::Paste(text)
            | egui::Event::Text(text)
            | egui::Event::Ime(egui::ImeEvent::Commit(text)) = &event
                && (!matches!(&event, egui::Event::Paste(_)) || !view.protocol_clipboard)
                && (text.len() > 32 * 1024 || !view.pending.is_empty())
            {
                tab.error =
                    Some("远程桌面正在发送输入，或单次文本超过 32 KiB，请稍后分批粘贴".into());
                continue;
            }
            match event {
                event @ (egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => {
                    if pointer::handle_surface(view, &event, &response, point, &mut send) {
                        response.request_focus();
                    }
                }
                egui::Event::MouseWheel { delta, .. } if response.hovered() => {
                    if let Some((x, y)) = view.position {
                        send(Input::Pointer {
                            x,
                            y,
                            button: None,
                            down: false,
                            wheel: if delta.y > 0.0 { 120 } else { -120 },
                        });
                    }
                }
                _ if focused => keyboard::handle(event, view, &mut send),
                _ => {}
            }
        }
        if focused {
            keyboard::modifiers(view, ui.input(|input| input.modifiers), &mut send);
        }
        if view.focused && !focused {
            keyboard::release(view, &mut send);
            if let Some((x, y)) = view.position {
                for button in 1..=3 {
                    if view.buttons & (1 << button) != 0 {
                        send(Input::Pointer {
                            x,
                            y,
                            button: Some(button),
                            down: false,
                            wheel: 0,
                        });
                    }
                }
            }
            view.buttons = 0;
        }
        view.focused = focused;
        view.queue_inputs(outgoing);
        // Preserve ordering and retry under backpressure instead of silently losing
        // the end of a paste (or a key-up) when the protocol queue fills.
        for _ in 0..32 {
            let Some(event) = view.pending.front().copied() else {
                break;
            };
            if session.send(event).is_err() {
                break;
            }
            view.pending.pop_front();
        }
        // Run the socket writer in this input frame; don't wait for another UI tick.
        session.poll();
        if !view.pending.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(5));
        }
    } else {
        view.pending.clear();
    }
    None
}
