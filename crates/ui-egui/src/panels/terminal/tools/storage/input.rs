use super::*;
pub(super) fn take(ui: &Ui, id: egui::Id, keys: &[egui::Key]) -> Option<egui::Key> {
    if !ui.is_enabled() || !ui.memory(|m| m.has_focus(id)) {
        return None;
    }
    ui.input_mut(|i| {
        keys.iter()
            .copied()
            .find(|key| i.consume_key(egui::Modifiers::NONE, *key))
    })
}
pub(super) fn offset(index: Option<usize>, len: usize, key: egui::Key) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match key {
        egui::Key::Home => 0,
        egui::Key::End => len - 1,
        egui::Key::ArrowUp => index.unwrap_or(1).saturating_sub(1),
        egui::Key::ArrowDown => index.map_or(0, |i| (i + 1).min(len - 1)),
        _ => return None,
    })
}
