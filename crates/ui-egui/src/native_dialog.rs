//! Shared painting for the original OpenJFX dialog buttons.
mod button_paint;
mod button_palette;
#[cfg(test)]
mod button_tests;
pub(crate) mod confirmation;
pub(crate) mod message;
pub(crate) mod placement;
pub(crate) mod spinner;
pub(crate) use button_paint::State as ButtonState;
use egui::Color32;
pub(crate) fn paint_button(ui: &egui::Ui, rect: egui::Rect, state: ButtonState) {
    button_paint::paint(ui, rect, state);
}
pub(crate) fn font(ctx: &egui::Context, size: f32) -> egui::FontId {
    static FAMILY: std::sync::LazyLock<egui::FontFamily> =
        std::sync::LazyLock::new(|| egui::FontFamily::Name("table".into()));
    let family = if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&*FAMILY)) {
        FAMILY.clone()
    } else {
        egui::FontFamily::Proportional
    };
    egui::FontId::new(size, family)
}
pub(crate) fn button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: &str,
    default: bool,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    button_paint::paint(
        ui,
        rect,
        button_paint::State {
            default,
            hover: response.hovered(),
            armed: response.is_pointer_button_down_on(),
            focused: response.has_focus(),
            enabled: ui.is_enabled(),
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font(ui.ctx(), 12.),
        Color32::BLACK.gamma_multiply(if ui.is_enabled() { 1. } else { 0.4 }),
    );
    response
}

pub(crate) fn gradient(ui: &egui::Ui, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    ui.painter().add(mesh);
}
