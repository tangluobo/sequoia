use super::*;
pub(super) fn render(
    ui: &mut Ui,
    connection: &mut tomato_core::Connection,
    session: &mut Option<tomato_services::remote_desktop::DesktopSession>,
    view: &mut crate::tabs::desktop::DesktopView,
    id: &str,
    error: &mut Option<String>,
) -> Option<Intent> {
    if let Some(session) = session.as_ref() {
        session.apply_adb_settings(connection);
    }
    let status = session
        .as_ref()
        .map(|s| s.status())
        .unwrap_or(Status::Closed);
    let old_size = view.size;
    let video_format = crate::gpu_video::format(ui.ctx());
    if let Some(session) = session.as_ref() {
        session.set_yuv_output(video_format.is_some());
    }
    if let Some(frame) =
        video_format.and_then(|_| session.as_ref().and_then(|s| s.take_yuv_snapshot()))
    {
        view.size = [frame.width, frame.height];
        if let Some(format) = video_format {
            if let Some(video) = &mut view.video {
                video.update(frame);
            } else {
                view.video = Some(crate::gpu_video::Video::new(frame, format));
                view.needs_focus = true;
            }
            view.texture = None;
        }
    }
    if let Some(frame) = session.as_ref().and_then(|s| s.take_rgba_frame()) {
        super::frame::upload_rgba(ui.ctx(), view, id, frame);
    } else if let Some(frame) = session.as_ref().and_then(|s| s.take_frame()) {
        super::frame::upload(ui.ctx(), view, id, frame);
    }
    if old_size != view.size {
        view.queue_inputs([Input::Resize {
            width: view.size[0],
            height: view.size[1],
        }]);
    }
    ui.spacing_mut().item_spacing = egui::vec2(4., 0.);
    ui.spacing_mut().button_padding = egui::vec2(6., 3.);
    ui.spacing_mut().interact_size.y = 24.;
    let mut reconnect = false;
    let mut configure = false;
    let mut disconnect = false;
    let mut toggle_window = false;
    let mut key = None;
    egui::Frame::new()
        .fill(Color32::from_gray(245))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(("adb-toolbar", id))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        toggle_window = ui
                            .button(if view.window.is_some() { "返回标签" } else { "独立窗口" })
                            .clicked();
                        ui.label(
                            RichText::new(match &status {
                                Status::Ready => format!(
                                    "{} · {}×{}",
                                    session
                                        .as_ref()
                                        .map(|s| s.desktop_name())
                                        .unwrap_or_else(|| connection.text("host").into()),
                                    view.size[0],
                                    view.size[1]
                                ),
                                Status::Failed(e) => format!("连接失败：{e}"),
                                Status::Closed => "已断开".into(),
                                _ => "正在连接…".into(),
                            })
                            .size(12.),
                        );
                        reconnect = ui.button("重新连接").clicked();
                        disconnect = ui.button("断开").clicked();
                        if ui
                            .selectable_label(
                                connection.text("adbDisplayMode") != "PNG",
                                "流畅视频",
                            )
                            .clicked()
                        {
                            connection.set("adbDisplayMode", "H264");
                            configure = true;
                        }
                        if ui
                            .selectable_label(
                                connection.text("adbDisplayMode") == "PNG",
                                "稳定截图",
                            )
                            .clicked()
                        {
                            connection.set("adbDisplayMode", "PNG");
                            configure = true;
                        }
                        use tomato_services::remote_desktop::AdbVideoQuality;
                        let mut quality = AdbVideoQuality::from_connection(connection);
                        let mut selected = false;
                        egui::ComboBox::from_id_salt(("adb-quality", id))
                            .width(100.)
                            .selected_text(quality.label())
                            .show_ui(ui, |ui| {
                                for preset in AdbVideoQuality::ALL {
                                    selected |= ui.selectable_value(&mut quality, preset, preset.label()).clicked();
                                }
                            })
                            .response
                            .on_hover_text("高清：长边最多 1920、24 Mbps；超清：2560、24 Mbps；原生：设备分辨率、32 Mbps。均保留 60 FPS 上限，设备及网络决定实际帧率。选择后自动重连，重新选择当前档位也会应用其新参数。");
                        if selected {
                            quality.apply(connection);
                            connection.set("adbDisplayMode", "H264");
                            configure = true;
                        }
                        if let Some(stats) = session.as_ref().and_then(|s| s.adb_video_stats()) {
                            ui.label(
                                RichText::new(if stats.frames_per_second > 0. {
                                    format!("{} · {:.0} FPS", stats.mode, stats.frames_per_second)
                                } else {
                                    stats.mode.into()
                                })
                                .size(12.),
                            );
                        }
                        for (label, code) in [
                            ("↩ 返回", 4),
                            ("⌂ Home", 3),
                            ("▣ 最近任务", 187),
                            ("⏻ 电源", 26),
                            ("音量−", 25),
                            ("音量+", 24),
                        ] {
                            if ui
                                .add_enabled(status == Status::Ready, egui::Button::new(label))
                                .clicked()
                            {
                                key = Some(code);
                            }
                        }
                        if ui
                            .add_enabled(status == Status::Ready, egui::Button::new("粘贴"))
                            .clicked()
                        {
                            view.focused = true;
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        }
                    });
                });
        });
    if disconnect {
        *session = None;
        view.reset_vnc_input();
    }
    if reconnect {
        return Some(Intent::ConnectDesktop);
    }
    if toggle_window {
        return Some(Intent::ToggleAdbWindow);
    }
    if configure {
        if let Some(session) = session.as_ref() {
            if let Err(problem) = session.reconfigure_adb(connection) {
                *error = Some(problem.to_string());
            }
        } else {
            return Some(Intent::ConnectDesktop);
        }
    }
    let bounds = ui.available_rect_before_wrap();
    ui.painter().rect_filled(bounds, 0, Color32::from_gray(22));
    let (rect, response) = if view.texture.is_some() || view.video.is_some() {
        super::vnc::surface(ui, bounds, view)
    } else {
        (
            bounds,
            ui.interact(
                bounds,
                ui.make_persistent_id("adb-empty"),
                egui::Sense::click(),
            ),
        )
    };
    if view.texture.is_none() && view.video.is_none() {
        ui.painter().text(
            bounds.center(),
            egui::Align2::CENTER_CENTER,
            "等待设备画面",
            egui::FontId::proportional(14.),
            Color32::LIGHT_GRAY,
        );
    }
    let mut queued = Vec::new();
    if !ui.input(|i| i.focused)
        || ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|point| !rect.contains(point))
        })
    {
        view.focused = false;
        super::keyboard::release(view, &mut |input| queued.push(input));
    }
    if let Some(_session) = session.as_ref().filter(|s| s.status() == Status::Ready) {
        if let Some(code) = key {
            queued.push(Input::AndroidKey(code));
        }
        if response.clicked() || response.drag_started() {
            view.focused = true;
            response.request_focus();
        }
        let size = view.size;
        let point = |p: egui::Pos2| {
            (
                ((p.x - rect.left()) / rect.width() * f32::from(size[0]))
                    .clamp(0., f32::from(size[0].saturating_sub(1))) as u16,
                ((p.y - rect.top()) / rect.height() * f32::from(size[1]))
                    .clamp(0., f32::from(size[1].saturating_sub(1))) as u16,
            )
        };
        let (events,overlong)=ui.input(|input|{
            let mut overlong=false;
            let events=input.events.iter().filter_map(|event|{
                if matches!(event,egui::Event::Text(text)|egui::Event::Paste(text)|egui::Event::Ime(egui::ImeEvent::Commit(text)) if text.chars().take(2049).count()>2048){overlong=true;None}else{Some(event.clone())}
            }).collect::<Vec<_>>();(events,overlong)
        });
        if overlong {
            *error = Some("ADB 每次最多粘贴 2048 字符，请分段粘贴".into());
        }
        for event in events {
            if view.pending.len() + queued.len() > 8192 {
                *error = Some("ADB 输入队列繁忙，请等待后再输入".into());
                continue;
            }
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if rect.contains(pos) || (!pressed && view.buttons != 0) => {
                    let b = match button {
                        egui::PointerButton::Primary => 1,
                        egui::PointerButton::Secondary => 2,
                        _ => continue,
                    };
                    let (x, y) = point(pos);
                    view.position = Some((x, y));
                    if pressed {
                        view.buttons |= 1 << b;
                        view.focused = true;
                    } else {
                        view.buttons &= !(1 << b);
                    }
                    queued.push(Input::Pointer {
                        x,
                        y,
                        button: Some(b),
                        down: pressed,
                        wheel: 0,
                    });
                }
                egui::Event::PointerMoved(pos) if view.buttons & (1 << 1) != 0 => {
                    let (x, y) = point(pos);
                    view.position = Some((x, y));
                    queued.push(Input::Pointer {
                        x,
                        y,
                        button: None,
                        down: false,
                        wheel: 0,
                    });
                }
                egui::Event::MouseWheel { delta, .. } if response.hovered() => {
                    let p = response.hover_pos().unwrap_or(rect.center());
                    let (x, y) = point(p);
                    queued.push(Input::Pointer {
                        x,
                        y,
                        button: None,
                        down: false,
                        wheel: if delta.y > 0. { 120 } else { -120 },
                    });
                }
                event if view.focused => super::keyboard::handle(event, view, &mut |input| {
                    queued.push(input);
                }),
                _ => {}
            }
        }
    }
    view.queue_inputs(queued);
    None
}
