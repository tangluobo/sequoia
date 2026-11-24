//! Original single information tab used before execution and while the actor is busy.
use egui::*;
pub(super) fn render(ui: &mut Ui, title: &str, message: &str, error: bool) {
    let outer = ui.available_rect_before_wrap();
    ui.painter().rect_filled(outer, 0., Color32::from_gray(244));
    crate::native_tabs::show(
        ui,
        "query-information-tab",
        &mut 0_u8,
        crate::native_tabs::Style::Compact,
        [(0, title)],
    );
    ui.label(RichText::new(message).size(12.).color(if error {
        Color32::from_rgb(204, 0, 0)
    } else {
        Color32::from_gray(136)
    }));
}
