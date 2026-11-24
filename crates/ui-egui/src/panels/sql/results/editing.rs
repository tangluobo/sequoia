use super::*;
pub(super) fn finish(ui: &Ui, result: &QueryResult, state: &mut State) -> bool {
    if let Some(editing) = &mut state.editing {
        editing.release(ui.ctx());
        if let Err(error) = editing.finish(result) {
            state.error = error.to_string();
            return false;
        }
    }
    true
}
pub(super) fn editor(
    ui: &mut Ui,
    result: &QueryResult,
    state: &mut State,
    cell: Cell,
    rect: Rect,
    id: Id,
) {
    let row = state.row(cell.row);
    let Some(editing) = &mut state.editing else {
        return;
    };
    if !editing.enabled {
        return;
    }
    let Some(editor) = &mut editing.editor else {
        return;
    };
    if editor.row != row || editor.column != cell.column {
        return;
    }
    let editor_id = editing
        .editor_state
        .prepare_id(ui.ctx(), id.with("cell-editor"));
    let focus = std::mem::take(&mut editor.focus);
    let kind = crate::tabs::query_results::temporal_model::Kind::jdbc(
        result
            .column_types
            .get(cell.column)
            .copied()
            .unwrap_or_default(),
    );
    let input_rect = Rect::from_min_max(
        rect.min,
        rect.max - egui::vec2(if kind.is_some() { 26. } else { 0. }, 0.),
    );
    let output = ui
        .scope_builder(
            egui::UiBuilder::new().max_rect(input_rect.shrink(2.)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(rect));
                ui.visuals_mut().selection.bg_fill = Color32::from_rgb(0, 150, 201);
                ui.visuals_mut().selection.stroke = Stroke::new(1., Color32::WHITE);
                ui.painter().rect_filled(rect, 0., Color32::WHITE);
                ui.painter().rect_stroke(
                    rect,
                    0.,
                    Stroke::new(2., Color32::from_rgb(53, 146, 203)),
                    egui::StrokeKind::Inside,
                );
                let family = if ui.ctx().fonts(|fonts| {
                    fonts
                        .definitions()
                        .families
                        .contains_key(&egui::FontFamily::Name("table".into()))
                }) {
                    egui::FontFamily::Name("table".into())
                } else {
                    egui::FontFamily::Proportional
                };
                egui::TextEdit::singleline(&mut editor.text)
                    .id(editor_id)
                    .font(FontId::new(10., family))
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 0)))
                    .vertical_align(egui::Align::Center)
                    .text_color(Color32::BLACK)
                    .desired_width((input_rect.width() - 4.).max(0.))
                    .min_size(input_rect.shrink(2.).size())
                    .margin(egui::vec2(4., 0.))
                    .show(ui)
            },
        )
        .inner;
    let response = output.response;
    if response.changed() {
        let original = editing
            .cells
            .get(&(row, cell.column))
            .map_or_else(|| result.value(row, cell.column), |value| value.as_deref());
        editor.null = original.is_none() && editor.text.is_empty();
        editor.changed = original
            != if editor.null {
                None
            } else {
                Some(editor.text.as_str())
            };
    }
    if focus {
        response.request_focus();
        let mut text_state = output.state;
        let end = egui::text::CCursor::new(editor.text.chars().count());
        text_state
            .cursor
            .set_char_range(Some(if std::mem::take(&mut editor.select_all) {
                egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)
            } else {
                egui::text::CCursorRange::one(end)
            }));
        text_state.store(ui.ctx(), editor_id);
    }
    if let Some(text_state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), editor_id) {
        text_state.trim_undo_memory(8 * 1024 * 1024);
    }
    if let Some(kind) = kind
        && super::temporal::picker_button(
            ui,
            Rect::from_min_max(
                egui::pos2(rect.right() - 26., rect.top() + 2.),
                rect.right_bottom() - egui::vec2(2., 2.),
            ),
            kind,
            id.with(("calendar", row, cell.column)),
        )
        .clicked()
    {
        if let Some(picker) = editor.temporal.take() {
            picker.release(ui.ctx());
        }
        let screen = ui.input(|input| {
            input
                .viewport()
                .inner_rect
                .map_or(egui::Pos2::ZERO, |rect| rect.min)
        });
        editor.temporal = Some(Box::new(crate::tabs::query_results::temporal::Picker::new(
            kind,
            &editor.text,
            response.rect.left_bottom() + screen.to_vec2() + egui::vec2(0., 5.),
        )));
    }
    let popup = editor.temporal.is_some();
    let cancel = !popup && ui.input(|input| input.key_pressed(Key::Escape));
    let finish =
        !popup && (ui.input(|input| input.key_pressed(Key::Enter)) || response.lost_focus());
    if cancel || finish {
        editing.release(ui.ctx());
    }
    if cancel {
        editing.editor = None;
    } else if finish && let Err(error) = editing.finish(result) {
        state.error = error.to_string();
    }
    if cancel || finish {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
}
pub(super) fn set_selected(ui: &Ui, result: &QueryResult, state: &mut State, value: Option<&str>) {
    let cells: Vec<_> = (0..state.visible_rows())
        .flat_map(|row| (0..result.columns.len()).map(move |column| Cell { row, column }))
        .filter(|cell| state.contains(*cell))
        .map(|cell| (state.row(cell.row), cell.column))
        .collect();
    if let Some(editing) = &mut state.editing {
        editing.release(ui.ctx());
        if let Err(error) = editing.finish(result) {
            state.error = error.to_string();
            return;
        }
        for (row, column) in cells {
            if editing.writable(column)
                && let Err(error) = editing.set(result, row, column, value.map(str::to_owned))
            {
                state.error = error.to_string();
                break;
            }
        }
    }
}
