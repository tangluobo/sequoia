//! PortMappingPane inline styles and connect-tree.css table metrics.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Button {
    Add,
    Start,
    RowStart,
    Stop,
    Neutral,
}

pub(super) fn button(
    ui: &mut Ui,
    rect: egui::Rect,
    id: egui::Id,
    label: &str,
    kind: Button,
    enabled: bool,
) -> egui::Response {
    // These buttons already have absolute bounds inside a toolbar or fixed-height row.
    // A layout scope would advance the parent cursor even though the button is placed.
    let mut button = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    button.set_clip_rect(rect.intersect(ui.clip_rect()));
    if !enabled {
        button.disable();
    }
    {
        let ui = &mut button;
        // JavaFX disabled controls use 40% opacity, including their background.
        if !ui.is_enabled() {
            ui.set_opacity(0.4);
        }
        let response = ui.interact(rect, id, egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        let (fill, text) = match kind {
            Button::Add => (Color32::from_rgb(7, 193, 96), Color32::WHITE),
            Button::Start | Button::RowStart => (Color32::from_rgb(25, 118, 210), Color32::WHITE),
            Button::Stop => (Color32::from_rgb(255, 87, 34), Color32::WHITE),
            Button::Neutral => (Color32::from_gray(245), Color32::from_gray(51)),
        };
        ui.painter().rect_filled(rect, 4., fill);
        if matches!(kind, Button::Neutral) {
            ui.painter().rect_stroke(
                rect,
                4.,
                egui::Stroke::new(1., Color32::from_gray(208)),
                egui::StrokeKind::Inside,
            );
        }
        let size = if matches!(kind, Button::Stop | Button::RowStart) {
            11.
        } else {
            12.
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font(ui.ctx(), size, matches!(kind, Button::Add)),
            text,
        );
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

pub(super) fn cell_text(
    ui: &Ui,
    rect: egui::Rect,
    align: egui::Align2,
    text: &str,
    font: egui::FontId,
    color: Color32,
) {
    if rect.width() <= 0. {
        return;
    }
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.replace(['\r', '\n'], " "), font, color);
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(
            align.align_size_within_rect(galley.size(), rect).min,
            galley,
            color,
        );
}

pub(super) fn font(ctx: &egui::Context, size: f32, bold: bool) -> egui::FontId {
    let family = egui::FontFamily::Name("table-bold".into());
    if bold && ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        egui::FontId::new(size, family)
    } else {
        crate::native_dialog::font(ctx, size)
    }
}
