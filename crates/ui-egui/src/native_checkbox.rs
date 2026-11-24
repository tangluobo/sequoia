//! OpenJFX tree checkbox: white square and black check/partial mark.
pub(crate) fn checkbox(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    checked: bool,
    partial: bool,
    icons: &mut crate::icons::Icons,
) -> Option<bool> {
    let response = ui.interact(rect, id, egui::Sense::click());
    let bounds = egui::Rect::from_min_size(rect.min + egui::vec2(0., 2.), egui::vec2(12., 12.));
    ui.painter().rect_stroke(
        bounds,
        0.,
        egui::Stroke::new(1., egui::Color32::from_gray(85)),
        egui::StrokeKind::Inside,
    );
    if partial {
        ui.painter()
            .rect_filled(bounds.shrink2(egui::vec2(2., 5.)), 0., egui::Color32::BLACK);
    } else if checked {
        icons.paint(ui, "upstream/openjfx/backup-check.svg", bounds);
    }
    response.clicked().then_some(!checked)
}
