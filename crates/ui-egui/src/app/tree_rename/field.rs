use super::*;
pub(in crate::app) fn render(ui: &mut Ui, state: &mut State, rect: egui::Rect) -> Option<Commit> {
    let draft = state.as_mut().filter(|d| d.due.is_none())?;
    draft.drawn = true;
    let id = draft.id();
    draft.editor.prepare_id(ui.ctx(), id);
    let fresh = draft.focus;
    if fresh {
        draft.editor.select(ui.ctx(), 0..draft.name.chars().count());
        draft.focus = false;
    }
    let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width().max(16.), 26.));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.spacing_mut().interact_size.y = 0.;
    let response = child.add_sized(
        rect.size(),
        TextEdit::singleline(&mut draft.name)
            .id(id)
            .font(crate::theme::java_font(13.))
            .margin(egui::vec2(4., 1.))
            .vertical_align(egui::Align::Center)
            .char_limit(1024)
            .text_color(Color32::BLACK)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1., Color32::from_rgb(7, 193, 96)))
                    .corner_radius(3.),
            ),
    );
    if let Some(text) = egui::TextEdit::load_state(ui.ctx(), id) {
        text.trim_undo_memory(128 * 1024);
        text.store(ui.ctx(), id);
    }
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new(("tree-name-field", &draft.key)),
            response.rect,
        )
    });
    if ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("tree-rename-native-probe")))
        .unwrap_or(false)
    {
        let trace = serde_json::json!({"inline":true,"focused":response.has_focus(),"popup":egui::Popup::is_any_open(ui.ctx()),"name":draft.name,"key":draft.key,"fieldRect":[response.rect.left(),response.rect.top(),response.rect.right(),response.rect.bottom()],"rowHeight":crate::theme::TREE_ROW_HEIGHT,"fieldHeight":response.rect.height()});
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("tree-rename-native-state"), trace));
    }
    let escape = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let enter = response.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let outside = !fresh
        && ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !response.rect.contains(p))
        });
    if escape || enter || response.lost_focus() || outside || !ui.input(|i| i.focused) {
        return finish(state, ui.ctx(), !escape);
    }
    None
}
