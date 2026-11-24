//! Modal file/management forms: scoped keyboard input, initial selection, Enter and Escape.
use super::*;
pub(super) fn show(ui: &mut Ui, dialog: &mut Dialog, blocked: bool) -> (bool, bool) {
    let (mut accept, mut cancel) = (false, false);
    egui::Modal::new(ui.make_persistent_id("ssh-dialog")).show(ui.ctx(), |ui| {
        ui.set_width(if storage_action(dialog).is_some() {
            420.
        } else {
            320.
        });
        ui.label(RichText::new(&dialog.title).strong());
        ui.separator();
        if matches!(dialog.kind, Form::Notice) {
            ui.add(
                egui::Label::new(
                    dialog
                        .fields
                        .first()
                        .map(String::as_str)
                        .unwrap_or_default(),
                )
                .wrap(),
            );
        }
        if storage_action(dialog).is_some() {
            accept |= storage_fields(ui, dialog, blocked);
        } else {
            for (index, (label, field)) in dialog.labels.iter().zip(&mut dialog.fields).enumerate()
            {
                ui.label(label);
                let response = ui.add(
                    egui::TextEdit::singleline(field)
                        .char_limit(32768)
                        .desired_width(f32::INFINITY),
                );
                if index == 0 && dialog.focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(field.chars().count()),
                            )));
                        state.store(ui.ctx(), response.id);
                    }
                }
                accept |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
        }
        let validation = storage_action(dialog)
            .and_then(Result::err)
            .map(|e| e.to_string())
            .or_else(|| {
                if let Some(Operation::Manage(action)) = &dialog.confirm {
                    management::command(action).err().map(|e| e.to_string())
                } else {
                    None
                }
            });
        if let Some(error) = &validation {
            ui.colored_label(Color32::RED, error);
        }
        if let Some(Operation::Manage(action)) = &dialog.confirm {
            match management::command(action) {
                Ok(command) => {
                    ui.add(egui::Label::new(RichText::new(command).monospace()).wrap());
                }
                Err(e) => {
                    ui.colored_label(Color32::RED, e.to_string());
                }
            }
        }
        ui.horizontal(|ui| {
            let button =
                ui.add_enabled(!blocked && validation.is_none(), egui::Button::new("确定"));
            if dialog.focus && dialog.labels.is_empty() {
                button.request_focus();
            }
            accept |= button.clicked();
            if !matches!(dialog.kind, Form::Notice) {
                cancel = ui.button("取消").clicked();
            }
        });
        dialog.focus = false;
        cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if dialog.labels.is_empty() {
            accept |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        }
        accept &= !blocked && validation.is_none();
    });
    (accept && !cancel, cancel)
}
fn storage_fields(ui: &mut Ui, dialog: &mut Dialog, blocked: bool) -> bool {
    let mut enter = false;
    ui.add_enabled_ui(!blocked, |ui| {
        egui::Grid::new("storage-fields")
            .spacing([8., 8.])
            .show(ui, |ui| {
                for (index, (label, value)) in
                    dialog.labels.iter().zip(&mut dialog.fields).enumerate()
                {
                    if matches!(dialog.kind, Form::Mount) && index == 3 {
                        ui.label("");
                        let mut checked = value == "true";
                        if ui.checkbox(&mut checked, label).changed() {
                            *value = checked.to_string();
                        }
                    } else {
                        ui.label(label);
                        if matches!(dialog.kind, Form::Mount) && index == 2 {
                            egui::ComboBox::from_id_salt("storage-filesystem")
                                .width(250.)
                                .selected_text(value.as_str())
                                .show_ui(ui, |ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                    for fs in ["不格式化", "ext4", "xfs"] {
                                        ui.selectable_value(value, fs.into(), fs);
                                    }
                                });
                        } else {
                            let response = ui.add(
                                egui::TextEdit::singleline(value)
                                    .desired_width(250.)
                                    .char_limit(32768),
                            );
                            if index == 0 && dialog.focus {
                                response.request_focus();
                                if let Some(mut state) =
                                    egui::TextEdit::load_state(ui.ctx(), response.id)
                                {
                                    state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(0),
                                            egui::text::CCursor::new(value.chars().count()),
                                        ),
                                    ));
                                    state.store(ui.ctx(), response.id);
                                }
                            }
                            enter |= response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        }
                    }
                    ui.end_row();
                }
            });
    });
    enter
}
