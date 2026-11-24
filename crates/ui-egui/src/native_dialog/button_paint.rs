//! Rounded Modena backgrounds, using only transient meshes and static palettes.
use super::button_palette::PALETTES;
use egui::{Color32, Rect};
pub(crate) struct State {
    pub default: bool,
    pub hover: bool,
    pub armed: bool,
    pub focused: bool,
    pub enabled: bool,
}
pub(super) fn paint(ui: &egui::Ui, rect: Rect, state: State) {
    let State {
        default,
        hover,
        armed,
        focused,
        enabled,
    } = state;
    let index = usize::from(default) * 8
        + usize::from(focused && enabled) * 4
        + usize::from(armed && enabled) * 2
        + usize::from(hover && enabled);
    let opacity = if enabled { 1. } else { 0.4 };
    let mut mesh = egui::Mesh::default();
    let mut tessellator = egui::epaint::Tessellator::new(
        ui.ctx().pixels_per_point(),
        egui::epaint::TessellationOptions::default(),
        [1, 1],
        Vec::new(),
    );
    for layer in PALETTES[index] {
        let [top, right, bottom, left] = layer.insets;
        let bounds = Rect::from_min_max(
            rect.min + egui::vec2(left, top),
            rect.max - egui::vec2(right, bottom),
        );
        let shape = egui::epaint::RectShape::filled(bounds, layer.radius, Color32::WHITE)
            .with_round_to_pixels(false);
        let before = mesh.vertices.len();
        tessellator.tessellate_rect(&shape, &mut mesh);
        for vertex in &mut mesh.vertices[before..] {
            let t = ((vertex.pos.y - bounds.top()) / bounds.height()).clamp(0., 1.);
            let color = |component: usize| {
                (f32::from(layer.top[component]) * (1. - t)
                    + f32::from(layer.bottom[component]) * t)
                    .round() as u8
            };
            let alpha = f32::from(vertex.color.a()) / 255.;
            vertex.color = Color32::from_rgba_unmultiplied(color(0), color(1), color(2), color(3))
                .gamma_multiply(alpha * opacity);
        }
    }
    ui.painter().add(mesh);
}
