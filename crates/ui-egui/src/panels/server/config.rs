use super::*;

pub(super) fn render(ui: &mut Ui, view: &mut ServerView) -> bool {
    let mut changed = false;
    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
    ui.spacing_mut().item_spacing.y = 6.;
    ui.spacing_mut().interact_size.y = 14.;
    ui.spacing_mut().button_padding.y = 4.;
    ui.label(RichText::new("基本配置").size(13.).strong());
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 10,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12., 12.);
            ui.horizontal(|ui| {
                caption(ui, "监听端口：");
                changed |= field(ui, &mut view.port, 160.);
                caption(ui, "绑定地址：");
                changed |= field(ui, &mut view.config.bind_address, 224.);
            });
            ui.horizontal(|ui| {
                ui.add_sized([60., 14.], egui::Label::new("允许匿名："));
                let label = if view.config.kind == ServerType::Smb {
                    "公开的只读共享允许匿名访问"
                } else {
                    "未登录用户可访问目录（只读）"
                };
                changed |= ui
                    .checkbox(&mut view.config.anonymous_access, label)
                    .changed();
            });
            ui.horizontal(|ui| {
                caption(ui, "根目录：");
                ui.spacing_mut().item_spacing.x = 6.;
                changed |= field(ui, &mut view.config.root_directory, 420.);
                if ui
                    .add_sized(
                        [42., 23.],
                        egui::Button::new(RichText::new("浏览").size(11.)),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    view.config.root_directory = path.display().to_string();
                    changed = true;
                }
            });
        });
    let hint = if view.config.kind == ServerType::Smb {
        "SMB 配置在下次启动时生效。445 端口已被占用时可改用其他端口（如 4445）。"
    } else {
        "提示：HTTP/FTP/SMB 的端口如被系统占用，请改用其他端口（如 HTTP:8080, FTP:2121, SMB:4445）"
    };
    ui.label(RichText::new(hint).size(10.).color(Color32::from_gray(153)));
    changed
}
fn caption(ui: &mut Ui, text: &str) {
    ui.add_sized([60., 23.], egui::Label::new(text));
}
fn field(ui: &mut Ui, text: &mut String, width: f32) -> bool {
    ui.add_sized(
        [width, 23.],
        TextEdit::singleline(text).font(egui::FontId::proportional(12.)),
    )
    .changed()
}
pub(super) fn tabs(ui: &mut Ui, view: &mut ServerView) {
    let (strip, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.), egui::Sense::hover());
    ui.painter()
        .rect_filled(strip, 0, Color32::from_rgb(247, 248, 250));
    for (index, label) in ["📁 共享目录", "👥 账号管理", "📋 服务日志"]
        .into_iter()
        .enumerate()
    {
        let rect = egui::Rect::from_min_size(
            strip.min + egui::vec2(index as f32 * 89., 0.),
            egui::vec2(89., 36.),
        );
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("server-page", index)),
            egui::Sense::click(),
        );
        if view.page == index {
            ui.painter().rect_filled(rect, 0, Color32::WHITE);
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(2., Color32::from_rgb(7, 193, 96)),
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.),
            Color32::from_gray(51),
        );
        if response.clicked() {
            view.page = index;
            view.clear_selection();
        }
    }
}
