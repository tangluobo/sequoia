use super::*;
use crate::tabs::query_results::temporal_model::Kind;

#[derive(Clone, Copy)]
pub(super) enum End {
    Commit,
    Cancel,
}

pub(super) fn kind(column: &Column, database: &str) -> Option<Kind> {
    let name = column.data_type.to_ascii_uppercase();
    let name = name.split('(').next().unwrap_or(&name).trim();
    if name.starts_with("TIMESTAMP") || name.starts_with("DATETIME") || name == "SMALLDATETIME" {
        Some(Kind::DateTime)
    } else if name.starts_with("TIME") {
        Some(Kind::Time)
    } else if name == "DATE" {
        Some(if database == "ORACLE" {
            Kind::DateTime
        } else {
            Kind::Date
        })
    } else {
        None
    }
}

fn same(editor: &CellEditor, value: Option<&Cell>) -> bool {
    if editor.default {
        return value.is_none();
    }
    if editor.null {
        return value == Some(&Cell::Null);
    }
    match value {
        Some(Cell::Text(text)) => editor.text == *text,
        Some(Cell::Binary(bytes)) => {
            editor.text.is_ascii()
                && editor.text.len() == bytes.len() * 2
                && bytes.iter().enumerate().all(|(index, byte)| {
                    u8::from_str_radix(&editor.text[index * 2..index * 2 + 2], 16).ok()
                        == Some(*byte)
                })
        }
        None | Some(Cell::Null) => false,
        Some(value) => editor.text == value.display(),
    }
}

pub(super) fn finish(ctx: &egui::Context, state: &mut Workbench, database: &str, end: End) -> bool {
    if matches!(end, End::Cancel) {
        state.batch = None;
    } else if let Err(error) = super::batch::finish(state, database) {
        state.status = format!("批量输入无效：{error}");
        return false;
    }
    let Some(mut editor) = state.editor.take() else {
        return true;
    };
    ctx.request_repaint();
    if matches!(end, End::Cancel) || !editor.changed {
        return true;
    }
    let value = (|| -> anyhow::Result<Option<Cell>> {
        if editor.default {
            return Ok(None);
        }
        if editor.null {
            return Ok(Some(Cell::Null));
        }
        Ok(Some(parse(
            state,
            editor.row,
            editor.column,
            &editor.text,
            editor.binary,
            database,
        )?))
    })();
    match value {
        Ok(value) => {
            set_value(state, editor.row, editor.column, value);
            true
        }
        Err(error) => {
            state.status = format!("输入无效：{error}");
            editor.focus = true;
            state.editor = Some(editor);
            ctx.request_repaint();
            false
        }
    }
}
pub(super) fn parse(
    state: &Workbench,
    row: usize,
    column: usize,
    text: &str,
    binary: bool,
    database: &str,
) -> anyhow::Result<Cell> {
    if binary {
        let text = text.trim();
        anyhow::ensure!(
            text.is_ascii() && text.len().is_multiple_of(2),
            "请输入偶数位十六进制内容"
        );
        let bytes = text
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair)?, 16).map_err(Into::into))
            .collect::<anyhow::Result<Vec<_>>>()?;
        return Ok(Cell::Binary(bytes));
    }
    let original = state
        .page
        .as_ref()
        .and_then(|page| page.rows.get(row))
        .and_then(|row| row.get(column));
    Ok(if database == "SQLITE" {
        match original {
            Some(Cell::Integer(_)) => Cell::Integer(text.parse()?),
            Some(Cell::Real(_)) => {
                let number: f64 = text.parse()?;
                anyhow::ensure!(number.is_finite(), "数值必须为有限数");
                Cell::Real(number)
            }
            Some(Cell::Boolean(_)) => Cell::Boolean(text.parse()?),
            _ => Cell::Text(text.into()),
        }
    } else {
        Cell::Text(text.into())
    })
}

/// Commit before another cell or toolbar receives its press, not while hovering.
pub(super) fn before_grid(ui: &Ui, state: &mut Workbench, database: &str) -> bool {
    let outside = state.editor.as_ref().is_some_and(|editor| {
        editor.rect.is_some_and(|rect| {
            ui.input(|input| {
                (input.pointer.primary_pressed() || input.pointer.secondary_pressed())
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|point| !rect.contains(point))
            })
        })
    });
    !outside || finish(ui.ctx(), state, database, End::Commit)
}

pub(super) fn cell(
    ui: &mut Ui,
    editor: &mut CellEditor,
    value: Option<&Cell>,
    temporal: Option<Kind>,
    rect: egui::Rect,
    font: egui::FontId,
    id: egui::Id,
) -> Option<End> {
    editor.context = Some(ui.ctx().clone());
    editor.id = Some(id);
    editor.rect = Some(rect);
    let input = egui::Rect::from_min_max(
        rect.min,
        rect.max - egui::vec2(if temporal.is_some() { 26. } else { 0. }, 0.),
    );
    let focus = std::mem::take(&mut editor.focus);
    let editor_id = editor.input.prepare_id(ui.ctx(), id.with("inline-editor"));
    if focus {
        ui.memory_mut(|memory| memory.request_focus(editor_id));
        let mut text_state =
            egui::widgets::text_edit::TextEditState::load(ui.ctx(), editor_id).unwrap_or_default();
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
    let output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(input.shrink(2.)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.visuals_mut().selection.bg_fill = Color32::from_rgb(0, 150, 201);
            ui.visuals_mut().selection.stroke = egui::Stroke::new(1., Color32::WHITE);
            ui.painter().rect_filled(rect, 0, Color32::WHITE);
            ui.painter().rect_stroke(
                rect,
                0,
                egui::Stroke::new(2., Color32::from_rgb(53, 146, 203)),
                egui::StrokeKind::Inside,
            );
            TextEdit::singleline(&mut editor.text)
                .id(editor_id)
                .font(font)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 0)))
                .vertical_align(egui::Align::Center)
                .text_color(Color32::BLACK)
                .desired_width(input.width() - 4.)
                .min_size(input.shrink(2.).size())
                .margin(egui::vec2(4., 0.))
                .show(ui)
        })
        .inner;
    if focus {
        // The entering mouse release belonged to the grid in the previous
        // frame. Retain focus after registering the new input this frame.
        output.response.request_focus();
        ui.ctx().request_repaint();
    }
    if output.response.changed() {
        editor.default = false;
        editor.null = value == Some(&Cell::Null) && editor.text.is_empty();
        editor.changed = !same(editor, value);
    }
    editor.input.trim(ui.ctx());
    if let Some(kind) = temporal
        && super::super::results::temporal::picker_button(
            ui,
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - 26., rect.top() + 2.),
                rect.right_bottom() - egui::vec2(2., 2.),
            ),
            kind,
            id.with("calendar"),
        )
        .clicked()
    {
        let screen = ui.input(|input| {
            input
                .viewport()
                .inner_rect
                .map_or(egui::Pos2::ZERO, |rect| rect.min)
        });
        if let Some(old) = editor.temporal.take() {
            old.release(ui.ctx());
        }
        editor.temporal = Some(Box::new(crate::tabs::query_results::temporal::Picker::new(
            kind,
            &editor.text,
            rect.left_bottom() + screen.to_vec2() + egui::vec2(0., 5.),
        )));
    }
    if editor.temporal.is_none() && !focus {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            return Some(End::Cancel);
        }
        if ui.input(|input| input.key_pressed(egui::Key::Enter)) || output.response.lost_focus() {
            return Some(End::Commit);
        }
    }
    None
}

pub(super) fn picker(ctx: &egui::Context, state: &mut Workbench) {
    let Some(editor) = &mut state.editor else {
        return;
    };
    let Some(mut picker) = editor.temporal.take() else {
        return;
    };
    let (accept, close) = super::super::results::temporal::show_picker(
        ctx,
        &mut picker,
        editor
            .id
            .unwrap_or_else(|| egui::Id::new("table-data-calendar")),
    );
    if accept {
        for index in 0..3 {
            picker.commit_time(index);
        }
        editor.text = picker.value.formatted();
        editor.null = false;
        editor.default = false;
        let value = state.page.as_ref().and_then(|page| {
            if editor.row < page.rows.len() {
                state
                    .edits
                    .get(&editor.row)
                    .unwrap_or(&page.rows[editor.row])
                    .get(editor.column)
            } else {
                state
                    .inserted
                    .get(editor.row - page.rows.len())
                    .and_then(|row| row.get(editor.column))
                    .and_then(Option::as_ref)
            }
        });
        editor.changed = !same(editor, value);
    }
    if accept || close {
        picker.release(ctx);
        editor.focus = true;
        ctx.request_repaint();
    } else {
        editor.temporal = Some(picker);
    }
}

pub(super) fn set_value(state: &mut Workbench, row: usize, column: usize, value: Option<Cell>) {
    let Some(page) = &state.page else {
        return;
    };
    if row < page.rows.len() {
        if let Some(value) = value {
            state
                .edits
                .entry(row)
                .or_insert_with(|| page.rows[row].clone())[column] = value;
        }
        if state.edits.get(&row) == Some(&page.rows[row]) {
            state.edits.remove(&row);
        }
    } else if let Some(values) = state.inserted.get_mut(row - page.rows.len()) {
        values[column] = value;
    }
}
