//! Original browser interaction: select, click again after 300 ms, edit the name in place.
use super::*;
use crate::tabs::ssh::{Pane, RenameDraft};
use std::time::Duration;
use tomato_services::terminal::ssh::tools::Entry;

pub(super) fn editing(draft: &Option<RenameDraft>, path: &str) -> bool {
    draft
        .as_ref()
        .is_some_and(|edit| edit.from == path && edit.due.is_none())
}

pub(super) fn prepare(ui: &Ui, pane: &mut Pane, mode: &str) {
    let Some(edit) = &pane.rename else { return };
    let context_changed =
        edit.context.0 != pane.path || edit.context.1 != pane.filter || edit.context.2 != mode;
    let missing = pane.file_entry(&edit.from).is_none();
    let pending_invalid = edit.due.is_some()
        && (!ui.is_enabled()
            || egui::Popup::is_any_open(ui.ctx())
            || !ui.input(|i| i.focused)
            || ui.input(|i| i.pointer.is_decidedly_dragging())
            || ui.input(|i| i.key_pressed(egui::Key::Escape))
            || if mode == "COLUMN" {
                pane.column_selected.as_deref() != Some(&edit.from) || pane.selected.len() > 1
            } else {
                pane.selected.len() != 1
                    || !pane
                        .selected
                        .iter()
                        .any(|i| pane.entries.get(*i).is_some_and(|e| e.path == edit.from))
            });
    if context_changed || missing || pending_invalid {
        pane.finish_file_rename(ui.ctx(), false);
        return;
    }
    let edit = pane.rename.as_mut().unwrap();
    edit.drawn = false;
    if let Some(due) = edit.due {
        let now = ui.input(|i| i.time);
        if now < due {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(due - now));
        } else if !ui.input(|i| i.pointer.any_down()) {
            edit.due = None;
            edit.focus = true;
        }
    }
}

/// Called before changing selection, so a first click on another row cannot rename it.
pub(super) fn click(
    ui: &Ui,
    draft: &mut Option<RenameDraft>,
    entry: &Entry,
    response: &egui::Response,
    context: (&str, &str, &str),
    was_single_selected: bool,
    allowed: bool,
) -> bool {
    // An outside press commits the active field at the end of this frame.
    // Preserve it even when the OS batches press and release into one frame.
    if draft.as_ref().is_some_and(|edit| edit.due.is_none()) {
        return false;
    }
    if response.double_clicked() || response.triple_clicked() || response.secondary_clicked() {
        if let Some(edit) = draft.take() {
            edit.release(ui.ctx());
        }
        return false;
    }
    if !response.clicked() {
        return false;
    }
    if let Some(edit) = draft.take() {
        edit.release(ui.ctx());
    }
    if allowed && was_single_selected && ui.input(|i| i.modifiers.is_none()) {
        *draft = Some(RenameDraft::new(
            entry,
            context,
            Some(ui.input(|i| i.time) + 0.3),
        ));
        ui.ctx().request_repaint_after(Duration::from_millis(300));
        return true;
    }
    false
}

/// Paint once, after all rows/tiles, so a long filename can extend over adjacent tiles.
pub(super) fn field(
    ui: &mut Ui,
    pane: &mut Pane,
    rect: egui::Rect,
    icon: bool,
) -> Option<Operation> {
    let edit = pane.rename.as_mut().filter(|edit| edit.due.is_none())?;
    edit.drawn = true;
    let font = egui::FontId::proportional(if icon { 11. } else { 12. });
    let rect = if icon {
        let width = ui
            .painter()
            .layout_no_wrap(edit.name.clone(), font.clone(), Color32::BLACK)
            .size()
            .x
            + 20.;
        let width = width
            .max(rect.width())
            .min(ui.clip_rect().width().max(rect.width()));
        let mut rect = egui::Rect::from_center_size(rect.center(), egui::vec2(width, 22.));
        rect = rect.translate(egui::vec2(
            (ui.clip_rect().left() - rect.left()).max(0.)
                - (rect.right() - ui.clip_rect().right()).max(0.),
            0.,
        ));
        rect
    } else {
        rect.shrink2(egui::vec2(1., 1.))
    };
    let id = edit
        .editor
        .prepare_id(ui.ctx(), ui.make_persistent_id(("file-rename", &edit.from)));
    let fresh = edit.focus;
    if fresh {
        edit.editor.select(ui.ctx(), 0..edit.name.chars().count());
        edit.focus = false;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.spacing_mut().interact_size.y = 0.;
    let response = child.add_sized(
        rect.size(),
        egui::TextEdit::singleline(&mut edit.name)
            .id(id)
            .font(font)
            .char_limit(1024)
            .margin(egui::vec2(4., 0.))
            .vertical_align(egui::Align::Center)
            .horizontal_align(if icon {
                egui::Align::Center
            } else {
                egui::Align::LEFT
            })
            .text_color(Color32::from_gray(51))
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1., Color32::from_rgb(51, 153, 255))),
            ),
    );
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new(("file-rename-field", &edit.from)),
            response.rect,
        )
    });
    if ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new("file-rename-native-probe")))
        .unwrap_or(false)
    {
        let trace = serde_json::json!({"inline":true,"mode":edit.context.2,"name":edit.name,
            "focused":response.has_focus(),"popup":egui::Popup::is_any_open(ui.ctx()),
            "field":[response.rect.left(),response.rect.top(),response.rect.width(),response.rect.height()]});
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("file-rename-native-state"), trace));
    }
    if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
        state.trim_undo_memory(128 * 1024);
        state.store(ui.ctx(), id);
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
        response.surrender_focus();
        return pane.finish_file_rename(ui.ctx(), !escape);
    }
    None
}

pub(super) fn finish_frame(ui: &Ui, pane: &mut Pane) {
    if pane
        .rename
        .as_ref()
        .is_some_and(|edit| edit.due.is_none() && !edit.drawn)
    {
        pane.finish_file_rename(ui.ctx(), false);
    }
}
