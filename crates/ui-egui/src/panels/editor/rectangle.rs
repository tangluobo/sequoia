//! Rectangular selection consumes only editor-owned events, before TextEdit sees them.
use crate::tabs::markdown::{RectangleGeometry, RectangleState};
use egui::{Color32, Context, Event, Id, Key, PointerButton, Pos2, Shape};
use tomato_core::markdown_edit::{Position, Rectangle};

fn hit(geometry: &RectangleGeometry, point: Pos2) -> Position {
    let point = point - geometry.origin;
    let index = geometry
        .galley
        .rows
        .partition_point(|row| row.pos.y + row.size.y < point.y)
        .min(geometry.galley.rows.len().saturating_sub(1));
    let mut position = Position {
        row: index,
        column: 0,
    };
    if let Some(row) = geometry.galley.rows.get(index) {
        for glyph in &row.glyphs {
            if point.x < row.pos.x + glyph.pos.x + glyph.advance_width / 2. {
                break;
            }
            position.column += glyph.chr.len_utf16();
        }
    }
    position
}
pub(super) fn before(ctx: &Context, id: Id, state: &mut RectangleState, editable: bool) {
    if !editable {
        state.dragging = false;
        return;
    }
    let events = ctx.input_mut(|input| std::mem::take(&mut input.events));
    let mut remaining = Vec::with_capacity(events.len());
    for event in events {
        let mut consumed = false;
        match &event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
            } => {
                if let Some(geometry) = &state.geometry
                    && geometry.clip.contains(*pos)
                {
                    state.selection = None;
                    if modifiers.alt {
                        let position = hit(geometry, *pos);
                        state.selection = Some(Rectangle {
                            start: position,
                            end: position,
                        });
                        state.dragging = true;
                        ctx.memory_mut(|memory| memory.request_focus(id));
                        if let Some(mut editor) =
                            egui::widgets::text_edit::TextEditState::load(ctx, id)
                        {
                            if let Some(range) = editor.cursor.char_range() {
                                editor
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(
                                        range.primary,
                                    )));
                            }
                            editor.store(ctx, id);
                        }
                        consumed = true;
                    }
                }
            }
            Event::PointerMoved(pos) if state.dragging => {
                if let (Some(selection), Some(geometry)) = (&mut state.selection, &state.geometry) {
                    selection.end = hit(geometry, *pos);
                }
                consumed = true;
            }
            Event::PointerButton {
                button: PointerButton::Primary,
                pressed: false,
                ..
            } if state.dragging => {
                state.dragging = false;
                if state.selection.is_some_and(|selection| !selection.active()) {
                    state.selection = None;
                }
                consumed = true;
            }
            Event::WindowFocused(false) => {
                state.dragging = false;
            }
            _ => {}
        }
        if !consumed {
            remaining.push(event);
        }
    }
    ctx.input_mut(|input| input.events = remaining);
}
pub(super) fn event_edit(
    event: &Event,
    text: &str,
    range: egui::text::CCursorRange,
    state: &mut RectangleState,
    status: &mut String,
    modifiers: egui::Modifiers,
) -> Option<egui::text_edit::EventEdit> {
    use tomato_core::markdown_edit::{continue_line, indent};
    let [left, right] = range.sorted_cursors();
    let mut handled = true;
    let result = match event {
        Event::Key {
            key: Key::Tab,
            pressed: true,
            modifiers,
            ..
        } => indent(text, left.index.into()..right.index.into(), modifiers.shift).map(Some),
        Event::Key {
            key: Key::Enter,
            pressed: true,
            modifiers,
            ..
        } if !modifiers.shift => continue_line(text, range.primary.index.into()),
        _ => {
            handled = false;
            Ok(None)
        }
    };
    let result = if handled {
        result
    } else {
        let selection = state.selection.filter(|selection| selection.active())?;
        match event {
            Event::Key {
                key: Key::Backspace | Key::Delete,
                pressed: true,
                ..
            } => selection.edit(text, "").map(|mut edit| {
                if edit.text == text {
                    edit.selection = left.index.into()..right.index.into();
                }
                Some(edit)
            }),
            Event::Text(value) | Event::Ime(egui::ImeEvent::Commit(value))
                if !modifiers.command
                    && !modifiers.alt
                    && !value.is_empty()
                    && !value.contains(['\r', '\n', '\t']) =>
            {
                selection.edit(text, value).map(Some)
            }
            Event::Ime(egui::ImeEvent::Preedit { .. }) => {
                return Some(egui::text_edit::EventEdit::Selection(range));
            }
            _ => return None,
        }
    };
    match result {
        Ok(Some(edit)) => {
            state.selection = None;
            let cursor = egui::text::CCursorRange::two(
                egui::text::CCursor::new(edit.selection.start),
                egui::text::CCursor::new(edit.selection.end),
            );
            Some(egui::text_edit::EventEdit::Replace(edit.text, cursor))
        }
        Ok(None) => None,
        Err(error) => {
            *status = error.to_string();
            Some(egui::text_edit::EventEdit::Selection(range))
        }
    }
}
pub(super) fn after(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    state: &mut RectangleState,
    background: egui::layers::ShapeIdx,
) {
    let rebuild = state
        .geometry
        .as_ref()
        .is_none_or(|geometry| !std::sync::Arc::ptr_eq(&geometry.galley, &output.galley));
    if rebuild {
        state.geometry = Some(RectangleGeometry {
            galley: output.galley.clone(),
            origin: output.galley_pos,
            clip: ui.clip_rect(),
        });
    }
    let Some(geometry) = &mut state.geometry else {
        return;
    };
    geometry.origin = output.galley_pos;
    geometry.clip = ui.clip_rect();
    let mut shapes = Vec::new();
    if let Some(selection) = state.selection.filter(|selection| selection.active()) {
        let rows = selection.rows();
        let columns = selection.columns();
        let top = ui.clip_rect().top() - geometry.origin.y;
        let start = geometry
            .galley
            .rows
            .partition_point(|row| row.pos.y + row.size.y < top);
        for (index, row) in geometry.galley.rows.iter().enumerate().skip(start) {
            if geometry.origin.y + row.pos.y > ui.clip_rect().bottom() {
                break;
            }
            if !rows.contains(&index) {
                continue;
            }
            let mut column = 0;
            let mut bounds = egui::Rect::NOTHING;
            for glyph in &row.glyphs {
                let next = column + glyph.chr.len_utf16();
                if column < columns.end && next > columns.start {
                    let point = geometry.origin + row.pos.to_vec2() + egui::vec2(glyph.pos.x, 0.);
                    bounds = bounds.union(egui::Rect::from_min_size(
                        point,
                        egui::vec2(glyph.advance_width, row.size.y),
                    ));
                }
                column = next;
            }
            if bounds.is_positive() {
                shapes.push(Shape::rect_filled(
                    bounds,
                    0.,
                    Color32::from_rgb(207, 227, 255),
                ));
            }
        }
    }
    ui.painter().set(background, Shape::Vec(shapes));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &Context,
        text: &mut String,
        state: &mut RectangleState,
        events: Vec<Event>,
        time: f64,
    ) {
        let id = Id::new("column-editor-test");
        ctx.memory_mut(|memory| memory.request_focus(id));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(600., 400.),
                )),
                events,
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                before(ui.ctx(), id, state, true);
                let background = ui.painter().add(Shape::Noop);
                let mut status = String::new();
                let modifiers = ui.input(|input| input.modifiers);
                let pointer_selection = state.selection.is_none();
                let mut layouter = super::super::highlight::layout;
                let mut editor = |event: &Event, text: &str, range: egui::text::CCursorRange| {
                    event_edit(event, text, range, state, &mut status, modifiers)
                };
                let output = egui::TextEdit::multiline(text)
                    .id(id)
                    .code_editor()
                    .desired_width(580.)
                    .desired_rows(15)
                    .event_editor(&mut editor, 32 * 1024 * 1024)
                    .layouter(&mut layouter)
                    .pointer_selection(pointer_selection)
                    .show(ui);
                if output.response.changed() {
                    state.selection = None;
                }
                if state.selection.is_some() {
                    assert!(
                        output
                            .state
                            .cursor
                            .char_range()
                            .is_none_or(|range| range.is_empty()),
                        "Column selection must not also select a linear range"
                    );
                }
                after(ui, &output, state, background);
                assert!(status.is_empty(), "{status}");
            },
        );
        output.textures_delta.clear();
    }
    fn key(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
    #[test]
    fn actual_editor_keeps_coalesced_typing_enter_indent_and_undo_in_event_order() {
        let ctx = Context::default();
        let mut text = "- ".to_owned();
        let mut state = RectangleState::default();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![
                Event::Text("x".into()),
                key(Key::Enter, egui::Modifiers::NONE),
                Event::Text("next".into()),
            ],
            1.,
        );
        assert_eq!(text, "- x\n- next");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Tab, egui::Modifiers::NONE)],
            2.,
        );
        assert_eq!(text, "- x\n- next    ");
        super::super::actions::apply(
            &ctx,
            Id::new("column-editor-test"),
            &mut text,
            super::super::actions::Action::Undo,
        );
        assert_eq!(text, "- x\n- next");
        super::super::actions::apply(
            &ctx,
            Id::new("column-editor-test"),
            &mut text,
            super::super::actions::Action::Redo,
        );
        assert_eq!(text, "- x\n- next    ");
    }
    #[test]
    fn actual_alt_drag_replaces_each_selected_line_and_can_undo_the_whole_edit() {
        let ctx = Context::default();
        let original = "abcd\nx\nlast";
        let mut text = original.to_owned();
        let mut state = RectangleState::default();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        let point = |index: usize, geometry: &RectangleGeometry| {
            geometry
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                + geometry.origin.to_vec2()
        };
        let start = point(1, state.geometry.as_ref().unwrap());
        let end = point(10, state.geometry.as_ref().unwrap());
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![
                Event::ModifiersChanged(egui::Modifiers::ALT),
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::ALT,
                },
            ],
            1.,
        );
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![
                Event::PointerMoved(end),
                Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::ALT,
                },
            ],
            2.,
        );
        assert!(
            state.selection.is_some_and(
                |selection| selection.rows() == (0..3) && selection.columns() == (1..3)
            )
        );
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![
                Event::ModifiersChanged(egui::Modifiers::NONE),
                Event::Text("中".into()),
            ],
            3.,
        );
        assert_eq!(text, "a中d\nx中\nl中t");
        super::super::actions::apply(
            &ctx,
            Id::new("column-editor-test"),
            &mut text,
            super::super::actions::Action::Undo,
        );
        assert_eq!(text, original);
    }
    #[test]
    fn ime_preedit_keeps_the_rectangle_until_unicode_commit() {
        let ctx = Context::default();
        let mut text = "abcd\nx\nlast".to_owned();
        let mut state = RectangleState::default();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        state.selection = Some(Rectangle {
            start: Position { row: 0, column: 1 },
            end: Position { row: 2, column: 3 },
        });
        // Real selection begins by deselecting the ordinary cursor range.
        let id = Id::new("column-editor-test");
        let mut editor = egui::widgets::text_edit::TextEditState::load(&ctx, id).unwrap();
        editor
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(0),
            )));
        editor.store(&ctx, id);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::Ime(egui::ImeEvent::Preedit {
                text: "zhong".into(),
                active_range_chars: None,
            })],
            1.,
        );
        assert_eq!(text, "abcd\nx\nlast");
        assert!(state.selection.is_some());
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::Ime(egui::ImeEvent::Commit("中".into()))],
            2.,
        );
        assert_eq!(text, "a中d\nx中\nl中t");
        assert!(state.selection.is_none());
    }
}
