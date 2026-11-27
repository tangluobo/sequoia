use super::*;
pub(super) fn render(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> (Option<String>, Option<String>) {
    let section = Section::Files;
    let standalone = workspace.standalone == Some(section);
    let object = tomato_services::files::object_copy::is_object(connection);
    let bucket_root = bucket_icon(
        connection,
        &workspace.panes.entry(section).or_default().path,
    )
    .is_some();
    let (mut action, mut navigate) = (None, None);
    if connection.kind() == "FTP" && workspace.file_view == "COLUMN" {
        workspace.file_view = "LIST".into();
    }
    if standalone {
        let pane = workspace.panes.entry(section).or_default();
        egui::Frame::new()
            .fill(Color32::from_gray(248))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut input = pane.path_input.take().unwrap_or_else(|| pane.path.clone());
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut input)
                            .desired_width((ui.available_width() - 420.).max(20.)),
                    );
                    if field.gained_focus()
                        && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id)
                    {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(input.chars().count()),
                            )));
                        state.store(ui.ctx(), field.id);
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        let path = input.trim();
                        if !path.is_empty() {
                            navigate = Some(if object {
                                format!("/{}", path.trim_matches('/'))
                            } else {
                                path.into()
                            });
                        }
                    } else if field.has_focus() {
                        pane.path_input = Some(input);
                    }
                    for (mode, label, tip) in [
                        ("ICON", "⊞", "图标视图"),
                        ("LIST", "≡", "列表视图"),
                        ("COLUMN", "⫶", "列视图（多级目录）"),
                    ] {
                        if mode == "COLUMN" && connection.kind() == "FTP" {
                            continue;
                        }
                        if ui
                            .selectable_label(workspace.file_view == mode, label)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            workspace.file_view = mode.into();
                        }
                    }
                    ui.label("|");
                    if ui.small_button("↑ 上级").clicked() {
                        navigate = Some(parent(&pane.path));
                    }
                    if ui.small_button("⟳ 刷新").clicked() {
                        action = Some("刷新".into());
                    }
                    if !bucket_root && ui.small_button("+ 新建目录").clicked() {
                        action = Some("新建目录".into());
                    }
                    if !bucket_root
                        && ui
                            .small_button(
                                RichText::new("↑ 上传").color(Color32::from_rgb(7, 193, 96)),
                            )
                            .clicked()
                    {
                        action = Some("上传文件...".into());
                    }
                    if bucket_root
                        && ui
                            .small_button(
                                RichText::new("+ 新建Bucket").color(Color32::from_rgb(7, 193, 96)),
                            )
                            .clicked()
                    {
                        action = Some("新建Bucket".into());
                    }
                });
            });
    } else {
        egui::Frame::new()
            .fill(Color32::from_gray(245))
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut workspace.follow_terminal, "跟随终端");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("↑").on_hover_text("上传文件").clicked() {
                            action = Some("上传文件...".into());
                        }
                        if ui.small_button("⟳").on_hover_text("刷新").clicked() {
                            action = Some("刷新".into());
                        }
                    });
                });
            });
    }
    let pane = workspace.panes.entry(section).or_default();
    if !standalone {
        ui.horizontal(|ui| {
            if nav_button(ui, icons, "ssh:root", "根目录").clicked() {
                navigate = Some("/".into());
            }
            if nav_button(ui, icons, "ssh:parent", "上级目录").clicked() {
                navigate = Some(parent(&pane.path));
            }
            let mut draft = pane.path_input.take().unwrap_or_else(|| pane.path.clone());
            let input = ui.add(
                egui::TextEdit::singleline(&mut draft).desired_width(ui.available_width().max(60.)),
            );
            if input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                if !draft.trim().is_empty() {
                    navigate = Some(draft.trim().into());
                }
            } else if input.has_focus() {
                pane.path_input = Some(draft);
            }
        });
    }
    (action, navigate)
}
