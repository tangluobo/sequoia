//! Original standalone PortPanel keeps its header, protocol filter and footer fixed.
use super::*;
#[cfg(test)]
mod tests;
pub(super) fn render(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let bounds = ui.max_rect();
    let top = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 30.));
    let footer = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), (bounds.bottom() - 18.).max(top.bottom())),
        bounds.max,
    );
    ui.painter().rect_filled(top, 0, Color32::from_gray(245));
    ui.painter().text(
        top.left_center() + egui::vec2(8., 0.),
        egui::Align2::LEFT_CENTER,
        "端口监听",
        crate::theme::java_font(12.),
        Color32::from_gray(51),
    );
    let refresh = egui::Rect::from_center_size(
        egui::pos2(top.right() - 16., top.center().y),
        egui::vec2(24., 24.),
    );
    let response = ui
        .interact(
            refresh,
            ui.make_persistent_id("ports-refresh"),
            egui::Sense::click(),
        )
        .on_hover_text("刷新");
    icons.paint(
        ui,
        "images/connect/refresh.png",
        egui::Rect::from_center_size(refresh.center(), egui::vec2(16., 16.)),
    );
    if !workspace.busy(Section::Ports) && response.clicked() {
        workspace.panes.entry(Section::Ports).or_default().refresh = true;
    }
    let body = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), top.bottom()),
        egui::pos2(bounds.right(), footer.top()),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
    child.set_clip_rect(body.intersect(ui.clip_rect()));
    let intent = table::render(&mut child, workspace, Section::Ports, connection);
    let status = workspace
        .panes
        .get(&Section::Ports)
        .map_or("就绪", |p| p.status.as_str());
    let status_width = ui
        .painter()
        .layout_no_wrap(
            status.into(),
            crate::theme::java_font(10.),
            Color32::from_gray(136),
        )
        .size()
        .x
        + 16.;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            footer.min,
            egui::vec2(status_width.min(footer.width()), footer.height()),
        ),
        0,
        Color32::from_gray(245),
    );
    ui.painter().with_clip_rect(footer).text(
        footer.left_center() + egui::vec2(8., 0.),
        egui::Align2::LEFT_CENTER,
        status,
        crate::theme::java_font(10.),
        Color32::from_gray(136),
    );
    intent
}
