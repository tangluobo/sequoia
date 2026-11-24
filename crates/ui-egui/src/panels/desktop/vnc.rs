use super::*;
pub(super) mod password;
mod typing;
pub(super) use typing::Typing;
pub(super) fn chrome(
    ui: &mut Ui,
    connection: &tomato_core::Connection,
    status: &Status,
    name: &str,
    size: [u16; 2],
) -> (egui::Rect, bool, bool) {
    let mut reconnect = false;
    let mut disconnect = false;
    egui::Frame::new()
        .fill(Color32::from_gray(244))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            ui.spacing_mut().button_padding = egui::vec2(6., 2.);
            ui.spacing_mut().interact_size.y = 23.;
            ui.horizontal(|ui| {
                let text = match status {
                    Status::Connecting | Status::PasswordRequired => format!(
                        "正在连接 {}:{}…",
                        connection.text("host"),
                        tomato_services::remote_desktop::vnc_port(connection)
                    ),
                    Status::Ready => format!("已连接 · {name} · {}×{}", size[0], size[1]),
                    Status::Closed => if name.is_empty() {
                        "已断开"
                    } else {
                        "连接已断开"
                    }
                    .into(),
                    Status::Failed(message) => format!("连接失败：{message}"),
                };
                let width = ui
                    .painter()
                    .layout_no_wrap(
                        text.clone(),
                        egui::FontId::proportional(13.),
                        Color32::from_gray(85),
                    )
                    .size()
                    .x
                    + 8.;
                let width = width.min((ui.available_width() - 225.).max(60.));
                ui.add_sized(
                    [width, 23.],
                    egui::Label::new(RichText::new(text).size(13.).color(Color32::from_gray(85)))
                        .truncate(),
                )
                .on_hover_text(match status {
                    Status::Failed(message) => message.as_str(),
                    _ => name,
                });
                reconnect = ui
                    .add_enabled(
                        !matches!(status, Status::Connecting | Status::PasswordRequired),
                        egui::Button::new(RichText::new("重新连接").size(12.))
                            .min_size(egui::vec2(64., 23.)),
                    )
                    .clicked();
                disconnect = ui
                    .add(
                        egui::Button::new(RichText::new("断开").size(12.))
                            .min_size(egui::vec2(40., 23.)),
                    )
                    .clicked();
                if ui
                    .add(
                        egui::Button::new(RichText::new("全屏").size(12.))
                            .min_size(egui::vec2(40., 23.)),
                    )
                    .clicked()
                {
                    let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
            });
        });
    let bounds = ui.available_rect_before_wrap();
    ui.painter().rect_filled(bounds, 0, Color32::from_gray(22));
    ui.allocate_rect(bounds, egui::Sense::hover());
    (bounds, reconnect, disconnect)
}

pub(super) fn paint_last_frame(
    ui: &Ui,
    bounds: egui::Rect,
    view: &crate::tabs::desktop::DesktopView,
) {
    if let Some(video) = &view.video {
        let remote = egui::vec2(f32::from(view.size[0]), f32::from(view.size[1]));
        let scale = (bounds.width() / remote.x).min(bounds.height() / remote.y);
        let rect = egui::Rect::from_center_size(bounds.center(), remote * scale);
        video.paint(ui, bounds, rect);
        return;
    }
    if let Some(texture) = &view.texture {
        let remote = egui::vec2(f32::from(view.size[0]), f32::from(view.size[1]));
        let scale = (bounds.width() / remote.x).min(bounds.height() / remote.y);
        let rect = egui::Rect::from_center_size(bounds.center(), remote * scale);
        ui.painter().with_clip_rect(bounds).image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    }
}

pub(super) fn surface(
    ui: &mut Ui,
    bounds: egui::Rect,
    view: &crate::tabs::desktop::DesktopView,
) -> (egui::Rect, egui::Response) {
    let remote = egui::vec2(f32::from(view.size[0]), f32::from(view.size[1]));
    let scale = (bounds.width() / remote.x).min(bounds.height() / remote.y);
    let rect = egui::Rect::from_center_size(bounds.center(), remote * scale);
    paint_last_frame(ui, bounds, view);
    let response = ui.interact(
        rect,
        ui.make_persistent_id("vnc-surface"),
        egui::Sense::click_and_drag(),
    );
    (rect, response)
}
