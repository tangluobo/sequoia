//! Original Java slider rail, thumb, tick marks and keyboard interaction.
use super::*;
pub(super) fn show(
    ui: &mut Ui,
    rect: egui::Rect,
    value: &mut u8,
    bounds: (u8, u8, u8),
    salt: &str,
) -> bool {
    let (minimum, maximum, major) = bounds;
    let mut response = ui.interact(rect, ui.id().with(salt), egui::Sense::click_and_drag());
    let before = *value;
    let left = rect.left() + 5.;
    let right = (rect.right() - 5.).max(left + 1.);
    if response.clicked() {
        response.request_focus();
    }
    if (response.clicked() || response.dragged())
        && let Some(point) = response.interact_pointer_pos()
    {
        *value = (f32::from(minimum)
            + (point.x - left) / (right - left) * f32::from(maximum - minimum))
        .round()
        .clamp(f32::from(minimum), f32::from(maximum)) as u8;
    }
    if response.has_focus() {
        ui.input(|input| {
            if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowDown) {
                *value = value.saturating_sub(1).max(minimum);
            }
            if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowUp) {
                *value = value.saturating_add(1).min(maximum);
            }
            if input.key_pressed(egui::Key::Home) {
                *value = minimum;
            }
            if input.key_pressed(egui::Key::End) {
                *value = maximum;
            }
        });
    }
    if before != *value {
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*value), salt));
    let painter = ui.painter();
    let y = rect.top() + 10.;
    let rail = egui::Rect::from_min_max(egui::pos2(left, y - 3.), egui::pos2(right, y + 3.));
    painter.rect(
        rail,
        3,
        Color32::from_gray(225),
        egui::Stroke::new(1., Color32::from_gray(185)),
        egui::StrokeKind::Inside,
    );
    let minor = (major / 2).max(1);
    for tick in minimum..=maximum {
        if !(tick - minimum).is_multiple_of(minor) {
            continue;
        }
        let x = left + f32::from(tick - minimum) / f32::from(maximum - minimum) * (right - left);
        let primary = (tick - minimum).is_multiple_of(major);
        painter.vline(
            x,
            y + 7.5..=y + if primary { 12.5 } else { 10.5 },
            egui::Stroke::new(1., Color32::from_gray(85)),
        );
        if primary {
            text(
                ui,
                egui::pos2(x, y + 15.),
                egui::Align2::CENTER_TOP,
                &tick.to_string(),
                10.,
                Color32::from_gray(85),
            );
        }
    }
    let x = left
        + f32::from(value.saturating_sub(minimum)) / f32::from(maximum - minimum) * (right - left);
    if response.has_focus() {
        painter.circle_stroke(
            egui::pos2(x, y),
            8.,
            egui::Stroke::new(1., Color32::from_rgb(144, 201, 232)),
        );
    }
    painter.circle_filled(
        egui::pos2(x, y),
        6.,
        Color32::from_gray(if response.dragged() { 220 } else { 244 }),
    );
    painter.circle_stroke(
        egui::pos2(x, y),
        6.,
        egui::Stroke::new(1., Color32::from_gray(168)),
    );
    before != *value
}
