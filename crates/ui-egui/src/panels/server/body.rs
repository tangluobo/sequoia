use super::*;
pub(super) fn render(ui: &mut Ui, view: &mut ServerView) {
    ui.spacing_mut().item_spacing = egui::vec2(8., 10.);
    ui.spacing_mut().button_padding = egui::vec2(12., 5.);
    ui.spacing_mut().interact_size.y = 27.;
    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
    if view.page == 2 {
        if ui.add(button("清空日志")).clicked() {
            view.logs.clear();
        }
        egui::Frame::new()
            .fill(Color32::from_gray(250))
            .stroke(egui::Stroke::new(1., Color32::from_gray(238)))
            .show(ui, |ui| {
                ScrollArea::both()
                    .id_salt("server-log")
                    .min_scrolled_height(230.)
                    .max_height(230.)
                    .auto_shrink([false, false])
                    .show_rows(ui, 16., view.logs.len(), |ui, rows| {
                        for row in rows {
                            ui.label(RichText::new(&view.logs[row]).monospace().size(12.));
                        }
                    });
            });
        return;
    }
    let account = view.page == 1;
    ui.horizontal(|ui| {
        if ui
            .add(
                button(if account {
                    "+ 添加账号"
                } else {
                    "+ 添加目录"
                })
                .fill(Color32::from_rgb(7, 193, 96)),
            )
            .clicked()
        {
            view.dialog = Some(if account {
                Edit::Account {
                    index: None,
                    value: ServerAccount::default(),
                }
            } else {
                Edit::Share {
                    index: None,
                    value: SharedDirectory::default(),
                    users: String::new(),
                }
            });
        }
        if ui.add(button("编辑")).clicked() {
            dialogs::edit_selected(view, account);
        }
        ui.label("  ");
        if ui.add(button("删除")).clicked() {
            remove_selected(view, account);
        }
    });
    let count = if account {
        view.config.accounts.len()
    } else {
        view.config.shared_directories.len()
    };
    let labels = if account {
        ["用户名", "密码", "启用", "主目录"]
    } else {
        ["别名", "本地路径", "只读", "允许用户"]
    };
    let widths = if account {
        [140., 100., 60., 320.]
    } else {
        [120., 340., 60., 160.]
    };
    let table_height = if account { 240. } else { 264. };
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., Color32::from_gray(238)))
        .corner_radius(4)
        .show(ui, |ui| {
            ScrollArea::both()
                .id_salt(("server-rows", account))
                .min_scrolled_height(table_height)
                .max_height(table_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let row_height = 26.;
                    let width = ui.available_width().max(widths.iter().sum());
                    let (bounds, _) = ui.allocate_exact_size(
                        egui::vec2(width, ((count + 1) as f32 * row_height).max(table_height)),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            bounds.min,
                            egui::vec2(widths.iter().sum(), row_height),
                        ),
                        0,
                        Color32::from_gray(245),
                    );
                    paint_row(ui, bounds.min, &widths, &labels, None);
                    let first = ((ui.clip_rect().top() - bounds.top()) / row_height)
                        .floor()
                        .max(1.) as usize
                        - 1;
                    let end = (((ui.clip_rect().bottom() - bounds.top()) / row_height)
                        .ceil()
                        .max(1.) as usize)
                        .min(count);
                    for index in first..end {
                        let rect = egui::Rect::from_min_size(
                            bounds.min + egui::vec2(0., (index + 1) as f32 * row_height),
                            egui::vec2(bounds.width(), row_height),
                        );
                        let response = ui.interact(
                            rect,
                            ui.make_persistent_id((account, index)),
                            egui::Sense::click(),
                        );
                        let selected = view.selection.contains(&index);
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0,
                                if selected {
                                    crate::theme::TREE_SELECTED
                                } else {
                                    crate::theme::TREE_HOVER
                                },
                            );
                        }
                        let (values, checked) = if account {
                            let row = &view.config.accounts[index];
                            (
                                [
                                    row.username.clone(),
                                    "••••••".into(),
                                    String::new(),
                                    row.home_directory.clone(),
                                ],
                                row.enabled,
                            )
                        } else {
                            let row = &view.config.shared_directories[index];
                            (
                                [
                                    row.alias.clone(),
                                    row.path.clone(),
                                    String::new(),
                                    if row.allowed_users.is_empty() {
                                        "所有用户".into()
                                    } else {
                                        row.allowed_users.join(", ")
                                    },
                                ],
                                row.read_only,
                            )
                        };
                        paint_row(
                            ui,
                            rect.min,
                            &widths,
                            &values.each_ref().map(String::as_str),
                            Some(checked),
                        );
                        if response.clicked() {
                            let modifiers = ui.input(|i| i.modifiers);
                            view.select(index, modifiers.command, modifiers.shift);
                        }
                        if response.double_clicked() {
                            view.select(index, false, false);
                            dialogs::edit_selected(view, account);
                        }
                    }
                });
        });
    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        remove_selected(view, account);
    }
    if account {
        ui.label(RichText::new("匿名访问无需账号；若勾选“允许匿名”，未登录用户可浏览目录。FTP/HTTP 均支持账号密码登录。SMB 由于 Windows 445 端口常被系统占用，可能需管理员权限启动。").size(11.).color(Color32::GRAY));
    }
}
fn remove_selected(view: &mut ServerView, account: bool) {
    if !view.selection.is_empty() {
        view.dialog = Some(Edit::Remove {
            indices: view.selection.iter().copied().collect(),
            account,
        });
    }
}
fn button(text: &str) -> egui::Button<'_> {
    let color = if text.starts_with('+') {
        Color32::WHITE
    } else if text == "删除" {
        Color32::from_rgb(211, 47, 47)
    } else {
        Color32::from_gray(51)
    };
    egui::Button::new(RichText::new(text).size(12.).color(color))
        .min_size(egui::vec2(0., 27.))
        .corner_radius(4)
}
fn paint_row(
    ui: &Ui,
    origin: egui::Pos2,
    widths: &[f32; 4],
    values: &[&str; 4],
    checked: Option<bool>,
) {
    let mut x = origin.x;
    for (index, (width, value)) in widths.iter().zip(values).enumerate() {
        let rect = egui::Rect::from_min_size(egui::pos2(x, origin.y), egui::vec2(*width, 26.));
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if index == 2
            && let Some(checked) = checked
        {
            let box_rect = egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(12.5, 0.),
                egui::vec2(13., 13.),
            );
            painter.rect(
                box_rect,
                2,
                Color32::WHITE,
                egui::Stroke::new(1., Color32::from_gray(180)),
                egui::StrokeKind::Inside,
            );
            if checked {
                painter.add(egui::Shape::line(
                    vec![
                        box_rect.min + egui::vec2(3., 6.),
                        box_rect.min + egui::vec2(5., 9.),
                        box_rect.min + egui::vec2(10., 3.),
                    ],
                    egui::Stroke::new(1.5, Color32::from_gray(51)),
                ));
            }
        } else {
            let header = checked.is_none();
            painter.text(
                if header {
                    rect.center()
                } else {
                    rect.left_center() + egui::vec2(6., 0.)
                },
                if header {
                    egui::Align2::CENTER_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                *value,
                egui::FontId::proportional(if header { 12. } else { 11. }),
                Color32::from_gray(51),
            );
        }
        painter.line_segment(
            [rect.right_top(), rect.right_bottom()],
            egui::Stroke::new(1., Color32::from_gray(230)),
        );
        x += width;
    }
}
