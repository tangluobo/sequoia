//! JavaFX terminal's 12 px scrollbar; borrows the existing history.
pub(super) fn scrollbar(ui: &egui::Ui, parser: &mut vt100::Parser, track: egui::Rect) {
    let history = parser.screen().history_len();
    if history == 0 {
        return;
    }
    let rows = usize::from(parser.screen().size().0);
    let height = (track.height() * rows as f32 / (history + rows) as f32)
        .max(18.)
        .min(track.height());
    let travel = (track.height() - height).max(1.);
    let top =
        track.top() + travel * (history - parser.screen().scrollback()) as f32 / history as f32;
    let thumb =
        egui::Rect::from_min_size(egui::pos2(track.left() + 2., top), egui::vec2(8., height));
    ui.painter()
        .rect_filled(track, 0, egui::Color32::from_gray(45));
    let response = ui.interact(
        track,
        ui.make_persistent_id("terminal-scrollbar"),
        egui::Sense::click_and_drag(),
    );
    ui.painter().rect_filled(
        thumb,
        0,
        egui::Color32::from_gray(if response.hovered() || response.dragged() {
            125
        } else {
            95
        }),
    );
    if response.dragged() {
        let id = response.id.with("drag-origin");
        let position = response.interact_pointer_pos().map_or(top, |point| point.y);
        if response.drag_started() {
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .map_or(position, |point| point.y);
            ui.data_mut(|data| data.insert_temp(id, (parser.screen().scrollback(), origin)));
        }
        let (initial, origin) = ui.data_mut(|data| {
            *data.get_temp_mut_or_insert_with(id, || (parser.screen().scrollback(), position))
        });
        let offset = (initial as f32 - (position - origin) * history as f32 / travel)
            .round()
            .clamp(0., history as f32) as usize;
        parser.set_scrollback(offset);
        ui.ctx().request_repaint();
    } else if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let offset = parser.screen().scrollback();
        parser.set_scrollback(if pointer.y < top {
            offset.saturating_add(rows)
        } else if pointer.y > top + height {
            offset.saturating_sub(rows)
        } else {
            offset
        });
        ui.ctx().request_repaint();
    }
}
