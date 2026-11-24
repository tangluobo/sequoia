//! Geometry and colors from the original rdp4j RdpPane status bar.
use egui::{Align2, Color32, FontId, Rect, RichText, Stroke, Ui};
use tomato_core::Connection;
use tomato_services::remote_desktop::Status;

pub(super) fn render(
    ui: &mut Ui,
    connection: &Connection,
    status: &Status,
    size: [u16; 2],
    show_status_bar: bool,
) -> Rect {
    let bounds = ui.available_rect_before_wrap();
    ui.allocate_rect(bounds, egui::Sense::hover());
    let body = if show_status_bar {
        Rect::from_min_max(
            bounds.min,
            egui::pos2(bounds.right(), (bounds.bottom() - 23.0).max(bounds.top())),
        )
    } else {
        bounds
    };
    let bar = Rect::from_min_max(egui::pos2(bounds.left(), body.bottom()), bounds.max);
    ui.painter().rect_filled(
        body,
        0,
        if matches!(status, Status::Ready) {
            Color32::BLACK
        } else {
            Color32::from_rgb(32, 32, 32)
        },
    );
    if !show_status_bar {
        return body;
    }
    ui.painter()
        .rect_filled(bar, 0, Color32::from_rgb(240, 240, 240));
    ui.painter().hline(
        bar.x_range(),
        bar.top(),
        Stroke::new(1.0, Color32::from_gray(204)),
    );
    let (text, color) = match status {
        Status::Connecting => ("连接中...".into(), Color32::from_rgb(255, 165, 0)),
        Status::PasswordRequired => ("等待密码...".into(), Color32::from_rgb(255, 165, 0)),
        Status::Ready => ("已连接".into(), Color32::from_rgb(0, 128, 0)),
        Status::Closed => ("未连接".into(), Color32::from_gray(128)),
        Status::Failed(error) => (format!("连接失败: {error}"), Color32::RED),
    };
    ui.painter()
        .circle_filled(egui::pos2(bar.left() + 13.0, bar.center().y), 5.0, color);
    let state = Rect::from_min_max(
        egui::pos2(bar.left() + 26.0, bar.top() + 4.0),
        egui::pos2((bar.left() + 246.0).min(bar.right()), bar.bottom() - 3.0),
    );
    ui.put(
        state,
        egui::Label::new(RichText::new(&text).size(11.0))
            .selectable(true)
            .truncate(),
    )
    .on_hover_text(text);
    let font = FontId::proportional(11.0);
    ui.painter().text(
        egui::pos2(state.right() + 8.0, bar.center().y),
        Align2::LEFT_CENTER,
        "|",
        font.clone(),
        Color32::from_gray(204),
    );
    let dimension = |actual, key, default| {
        if actual > 0 {
            u64::from(actual)
        } else {
            match connection.number(key) {
                0 => default,
                value => value,
            }
        }
    };
    let width = dimension(size[0], "screenWidth", 1024);
    let height = dimension(size[1], "screenHeight", 768);
    let depth = match connection.number("colorDepth") {
        0 => 24,
        depth => depth,
    };
    let resolution = format!("{width}x{height} @{depth}");
    ui.painter().text(
        egui::pos2(bar.right() - 8.0, bar.center().y),
        Align2::RIGHT_CENTER,
        resolution,
        font.clone(),
        Color32::from_gray(102),
    );
    ui.painter()
        .with_clip_rect(Rect::from_min_max(
            egui::pos2(state.right() + 18.0, bar.top()),
            egui::pos2(
                (bar.right() - 130.0).max(state.right() + 18.0),
                bar.bottom(),
            ),
        ))
        .text(
            egui::pos2(state.right() + 20.0, bar.center().y),
            Align2::LEFT_CENTER,
            format!(
                "{}@{}:{}",
                connection.text("username"),
                connection.text("host"),
                connection.number("port")
            ),
            font,
            Color32::from_gray(51),
        );
    body
}
