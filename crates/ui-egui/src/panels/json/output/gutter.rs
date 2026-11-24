//! Measured original paragraph graphics:38px numbers,2px gaps,16/18px icons.
use super::*;
pub(super) enum Action {
    Toggle(u32),
    Copy(u32),
}
pub(super) fn show(
    ui: &Ui,
    id: egui::Id,
    rect: Rect,
    row: usize,
    node: Option<(u32, bool)>,
) -> Option<Action> {
    ui.painter().text(
        egui::pos2(rect.left() + 34., rect.center().y),
        Align2::RIGHT_CENTER,
        (row + 1).to_string(),
        FontId::monospace(12.),
        Color32::from_gray(170),
    );
    let (line, folded) = node?;
    let mut action = None;
    for (offset, width, label, tooltip, is_copy) in [
        (40., 16., if folded { "▶" } else { "▼" }, "展开/折叠", false),
        (58., 18., "📋", "复制节点为JSON", true),
    ] {
        let full = Rect::from_min_size(
            egui::pos2(rect.left() + offset, rect.top()),
            egui::vec2(width, rect.height()),
        );
        // Java's fixed70px HBox lets the last graphic overflow; keep the same
        // drawing position without taking clicks from the first text glyph.
        let hit = ui
            .interact(full.intersect(rect), id.with((line, is_copy)), Sense::CLICK)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tooltip);
        ui.painter().text(
            full.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(12.),
            if hit.hovered() {
                Color32::from_rgb(25, 118, 210)
            } else if is_copy {
                Color32::from_gray(144)
            } else {
                Color32::from_gray(128)
            },
        );
        if hit.clicked() {
            ui.memory_mut(|m| m.request_focus(id));
            action = Some(if is_copy {
                Action::Copy(line)
            } else {
                Action::Toggle(line)
            });
        }
    }
    action
}
