//! Column adjustments retain the Java fixed row height and adjacent-column policy.
use super::*;
use crate::tabs::ssh::Pane;
pub(super) fn widths(pane: &Pane, available: f32, standalone: bool) -> Vec<f32> {
    let mut widths = if standalone {
        vec![(available - 360.).max(120.), 100., 180., 80.]
    } else {
        vec![(available - 90.).max(100.), 70.]
    };
    if let Some(saved) = &pane.file_widths
        && saved.len() == widths.len()
    {
        widths.copy_from_slice(saved);
        widths[0] = (widths[0] + available - widths.iter().sum::<f32>()).max(60.);
    }
    widths
}
pub(super) fn resize(ui: &mut Ui, pane: &mut Pane, widths: &mut [f32], left: f32, top: f32) {
    let mut x = left;
    for column in 0..widths.len() - 1 {
        x += widths[column];
        let handle = egui::Rect::from_center_size(egui::pos2(x, top + 13.), egui::vec2(7., 26.));
        let response = ui
            .interact(
                handle,
                ui.make_persistent_id(("file-column-resize", column)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let start_id = response.id.with("start-widths");
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(start_id, [widths[column], widths[column + 1]]));
        }
        if response.dragged() {
            let start = ui
                .data(|data| data.get_temp::<[f32; 2]>(start_id))
                .unwrap_or([widths[column], widths[column + 1]]);
            let delta = response
                .total_drag_delta()
                .unwrap_or_default()
                .x
                .clamp(40. - start[0], start[1] - 40.);
            widths[column] = start[0] + delta;
            widths[column + 1] = start[1] - delta;
            let saved = pane
                .file_widths
                .get_or_insert_with(|| widths.to_vec().into_boxed_slice());
            saved.copy_from_slice(widths);
        }
        if response.drag_stopped() {
            ui.data_mut(|data| data.remove::<[f32; 2]>(start_id));
        }
    }
}
