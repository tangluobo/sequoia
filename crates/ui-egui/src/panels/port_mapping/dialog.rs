use super::*;
pub(super) fn show(ctx: &egui::Context, state: &mut State, hosts: &[tomato_core::Connection]) {
    let status = state.status.clone();
    let mut save = false;
    let mut cancel = false;
    if let Some(editor) = &mut state.editor {
        let viewport = egui::ViewportId::from_hash_of("port-mapping-editor");
        let size = egui::vec2(420., 377.);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(if editor.editing.is_some() {
                "修改端口映射"
            } else {
                "添加端口映射"
            })
            .with_inner_size(size)
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false);
        if editor.focus
            && let Some(owner) = ctx.input(|i| i.viewport().inner_rect)
        {
            builder = builder.with_position(owner.center() - size * 0.5);
        }
        let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
            && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("port_mapping_edit");
        if capture {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
        }
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            if capture
                && let Some(image) = ui.input(|i| {
                    i.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot {
                            viewport_id, image, ..
                        } if *viewport_id == viewport => Some(image.clone()),
                        _ => None,
                    })
                })
            {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-image"), image));
            }
            cancel |=
                ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
            let result = editor_view(ui, editor, hosts, &status);
            save |= result.0;
            cancel |= result.1;
            editor.focus = false;
        });
    }

    if cancel {
        state.editor = None;
    }
    if save && let Some(editor) = &mut state.editor {
        editor.mapping.local_port = editor.local.trim().parse().unwrap_or(0);
        editor.mapping.remote_port = editor.remote.trim().parse().unwrap_or(0);
        editor.mapping.name = editor.mapping.name.trim().into();
        for host in [
            &mut editor.mapping.local_host,
            &mut editor.mapping.remote_host,
        ] {
            *host = if host.trim().is_empty() {
                "127.0.0.1".into()
            } else {
                host.trim().into()
            };
        }
        match editor.mapping.validate() {
            Err(e) => state.status = e.to_string(),
            Ok(()) => {
                let mut next = state.mappings.clone();
                if let Some(i) = editor.editing {
                    next[i] = editor.mapping.clone();
                } else {
                    next.push(editor.mapping.clone());
                }
                if state.save(next) {
                    state.editor = None;
                    state.status = "已保存".into();
                }
            }
        }
    }
    let mut confirm = false;
    let mut cancel_delete = false;
    if let Some(ids) = &state.delete {
        egui::Window::new("删除端口映射")
            .id(egui::Id::new("delete-port-mappings"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("确定删除选中的 {} 条端口映射？", ids.len()));
                ui.horizontal(|ui| {
                    confirm = ui.button("确定").clicked();
                    cancel_delete = ui.button("取消").clicked();
                });
            });
    }
    if confirm && let Some(ids) = state.delete.clone() {
        let mut next = state.mappings.clone();
        next.retain(|m| !ids.contains(&m.id));
        if state.save(next) {
            for id in ids {
                state.stop(&id);
            }
            state.selected.clear();
            state.delete = None;
        }
    }
    if cancel_delete {
        state.delete = None;
    }

    let mut reverse = None;
    if let Some(index) = state.reverse.front().copied() {
        let m = &state.mappings[index];
        egui::Window::new("R 方向启动确认").collapsible(false).resizable(false).show(ctx,|ui|{
            ui.label(format!("R 方向会在 SSH 服务器的 {}:{} 上监听，经隧道连接本机 {}:{}。\n如果要在本地访问服务器的服务，请改用 L 方向。\n确认要继续用 R 方向启动吗？",m.remote_host,m.remote_port,m.local_host,m.local_port));
            ui.horizontal(|ui|{if ui.button("否").clicked(){reverse=Some(false);}
 if ui.button("是").clicked(){reverse=Some(true);}});
        });
    }
    if let Some(confirmed) = reverse
        && let Some(index) = state.reverse.pop_front()
        && confirmed
    {
        state.start(index, hosts, ctx);
    }
}

fn green_button(ui: &mut Ui, rect: egui::Rect, label: &str) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        4.,
        if response.hovered() {
            Color32::from_rgb(6, 173, 86)
        } else {
            Color32::from_rgb(7, 193, 96)
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::WHITE,
    );
    response
}
fn editor_view(
    ui: &mut Ui,
    editor: &mut Editor,
    hosts: &[tomato_core::Connection],
    status: &str,
) -> (bool, bool) {
    let origin = ui.max_rect().min;
    let at = |x, y, w, h| egui::Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
    ui.painter().rect_filled(ui.max_rect(), 0, Color32::WHITE);
    ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 12.));
    ui.spacing_mut().item_spacing = egui::vec2(8., 0.);
    ui.spacing_mut().interact_size.y = 23.;
    ui.spacing_mut().button_padding = egui::vec2(6., 3.);
    for (i, label) in [
        "SSH 主机:",
        "方向:",
        "名称 (可选):",
        "本地主机:",
        "本地端口:",
        "远程主机:",
        "远程端口:",
    ]
    .into_iter()
    .enumerate()
    {
        ui.painter().text(
            origin + egui::vec2(16., 27.5 + i as f32 * 33.),
            egui::Align2::LEFT_CENTER,
            label,
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::BLACK,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(at(87., 16., 280., 24.))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    egui::ComboBox::from_id_salt("mapping-host")
        .width(280.)
        .truncate()
        .selected_text(
            hosts
                .iter()
                .find(|h| h.id == editor.mapping.ssh_host_id)
                .map(|h| format!("{} ({}:{})", h.name, h.text("host"), h.number("port")))
                .unwrap_or_else(|| "请选择 SSH 主机".into()),
        )
        .show_ui(&mut child, |ui| {
            for h in hosts.iter().filter(|h| matches!(h.kind(), "SSH" | "SFTP")) {
                ui.selectable_value(
                    &mut editor.mapping.ssh_host_id,
                    h.id.clone(),
                    format!("{} ({}:{})", h.name, h.text("host"), h.number("port")),
                );
            }
        });
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(at(87., 49., 162., 24.))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    egui::ComboBox::from_id_salt("mapping-direction")
        .width(162.)
        .truncate()
        .selected_text(if editor.mapping.direction == "L" {
            "本地访问远程服务 (L)"
        } else {
            "远程访问本地服务 (R)"
        })
        .show_ui(&mut child, |ui| {
            ui.selectable_value(
                &mut editor.mapping.direction,
                "L".into(),
                "本地访问远程服务 (L)",
            );
            ui.selectable_value(
                &mut editor.mapping.direction,
                "R".into(),
                "远程访问本地服务 (R)",
            );
        });
    for (i, value, hint) in [
        (0, &mut editor.mapping.name, "如 MySQL跳板"),
        (1, &mut editor.mapping.local_host, ""),
        (2, &mut editor.local, "如 13306"),
        (3, &mut editor.mapping.remote_host, ""),
        (4, &mut editor.remote, "如 3306"),
    ] {
        ui.place(
            at(87., 82. + i as f32 * 33., 280., 23.),
            egui::TextEdit::singleline(value)
                .font(crate::native_dialog::font(ui.ctx(), 12.))
                .hint_text(hint)
                .margin(egui::vec2(6., 3.)),
        );
    }
    let hint = if editor.mapping.direction == "L" {
        "L：本机监听 本地主机:本地端口 → 经 SSH → 连接远程主机:远程端口\n场景：本地连远程数据库/网站。如本地 4444 → 服务器 80，本机浏览器访问 localhost:4444 即可连到服务器 80\n远程主机填服务在服务器上监听的地址（如 127.0.0.1 或 ::1）\n可保存多条使用同一本地端口的方案，但同一时刻只能启动其中一条"
    } else {
        "R：SSH 服务器监听 远程主机:远程端口 → 经隧道 → 连接本机 本地主机:本地端口\n场景：把本地服务暴露给远程。如本地 8080 → 服务器 9090，服务器上访问 9090 即连到本地 8080\n远程主机填 SSH 服务器上的绑定地址（如 127.0.0.1）；服务器端口需未被占用\n可保存多条重复方案；同一 SSH 服务器的同一远程端口不能同时启动"
    };
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(at(16., 247., 388., 79.))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    )
    .add(
        egui::Label::new(
            RichText::new(hint)
                .font(crate::native_dialog::font(ui.ctx(), 11.))
                .color(Color32::from_gray(136)),
        )
        .wrap(),
    );
    if status.contains("失败") || status.contains("请选择") || status.contains("端口必须")
    {
        ui.painter().text(
            origin + egui::vec2(16., 364.),
            egui::Align2::LEFT_CENTER,
            status,
            crate::native_dialog::font(ui.ctx(), 11.),
            Color32::RED,
        );
    }
    let cancel =
        crate::native_dialog::button(ui, at(314., 336., 42., 26.), "取消", false).clicked();
    let save = green_button(
        ui,
        at(364., 337., 40., 24.),
        if editor.editing.is_some() {
            "保存"
        } else {
            "确定"
        },
    )
    .clicked();
    (save, cancel)
}
