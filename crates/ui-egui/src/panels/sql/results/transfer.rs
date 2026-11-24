use super::*;
use tomato_core::result_transfer::Format;
fn copy_as(ui: &Ui, result: &QueryResult, state: &mut State, kind: Format<'_>) {
    match state.formatted(result, kind, false) {
        Ok(text) if !text.is_empty() => {
            state.error.clear();
            ui.ctx().copy_text(text);
        }
        Ok(_) => {}
        Err(error) => state.error = error.to_string(),
    }
}
pub(super) fn menu(ui: &mut Ui, result: &QueryResult, state: &mut State, id: Id, writable: bool) {
    ui.menu_button("复制为", |ui| {
        let table = state.source_table.clone();
        if ui
            .add_enabled(table.is_some(), egui::Button::new("Insert 语句"))
            .clicked()
        {
            copy_as(
                ui,
                result,
                state,
                Format::Insert {
                    table: table.as_deref().unwrap_or_default(),
                },
            );
            ui.close();
        }
        if ui
            .add_enabled(false, egui::Button::new("Update 语句"))
            .clicked()
        {
            copy_as(
                ui,
                result,
                state,
                Format::Update {
                    table: table.as_deref().unwrap_or_default(),
                    primary_keys: &[],
                },
            );
            ui.close();
        }
        ui.separator();
        ui.menu_button("JSON 格式", |ui| {
            for (label, camel) in [("原字段", false), ("字段转驼峰", true)] {
                if ui.button(label).clicked() {
                    copy_as(ui, result, state, Format::Json { camel });
                    ui.close();
                }
            }
        });
        if ui.button("注释：值").clicked() {
            copy_as(ui, result, state, Format::Comments);
            ui.close();
        }
        ui.separator();
        for (label, headers, data) in [
            ("制表符分隔值（数据）", false, true),
            ("制表符分隔值（字段名）", true, false),
            ("制表符分隔值（字段名和数据）", true, true),
        ] {
            if ui.button(label).clicked() {
                copy_as(ui, result, state, Format::Tsv { headers, data });
                ui.close();
            }
        }
    });
    if ui
        .add_enabled(writable, egui::Button::new("粘贴"))
        .clicked()
    {
        ui.memory_mut(|memory| memory.request_focus(id));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        ui.close();
    }
    ui.separator();
    if ui.button("保存数据为...").clicked() {
        state.export_requested = true;
        ui.close();
    }
}
