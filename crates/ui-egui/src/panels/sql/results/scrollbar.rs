//! Preserve the grid scroll IDs while sharing native scrollbar behavior.
pub(super) use crate::native_scrollbar::Metrics;
pub(super) fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vertical: bool,
    offset: &mut f32,
    metrics: Metrics,
    id: egui::Id,
) {
    let mut state = ui
        .ctx()
        .data(|data| data.get_temp::<crate::native_scrollbar::State>(id))
        .unwrap_or_default();
    crate::native_scrollbar::show(ui, rect, vertical, offset, metrics, id, &mut state);
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}
