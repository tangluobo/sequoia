//! Read-only TextEdit preserves whole-log selection/copy without undo snapshots.
use super::draft::Draft;
pub(super) fn render(ui: &mut egui::Ui, draft: &mut Draft) {
    let cache = &mut draft.log_text;
    let error = draft.error.as_deref();
    draft.control.read(|progress| {
        let lines = || progress.logs.iter().map(String::as_str).chain(error);
        let mut offset = 0usize;
        let unchanged = lines().all(|line| {
            let end = offset.saturating_add(line.len());
            let equal = cache.as_bytes().get(offset..end) == Some(line.as_bytes())
                && cache.as_bytes().get(end) == Some(&b'\n');
            offset = end.saturating_add(1);
            equal
        }) && offset == cache.len();
        if !unchanged {
            cache.clear();
            for line in lines() {
                cache.push_str(line);
                cache.push('\n');
            }
        }
    });
    let id = draft.editors[2].prepare_id(ui.ctx(), egui::Id::new(("backup-log-text", &draft.id)));
    let mut readonly = draft.log_text.as_str();
    let width = ui.available_width();
    let height = ui.available_height();
    let scroll = egui::ScrollArea::both()
        .id_salt("backup-log-scroll")
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add_sized(
                [width, height],
                egui::TextEdit::multiline(&mut readonly)
                    .id(id)
                    .font(egui::FontId::monospace(12.))
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(width, height)),
            );
        });
    draft.scroll_areas[2] = Some(scroll.id);
    draft.editors[2].trim(ui.ctx());
}
