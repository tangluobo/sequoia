use crate::tabs::completion::State;
use egui::text_edit::EventEdit;
use egui::{
    Color32, Event, Key, Ui,
    text::{CCursor, CCursorRange},
};
use tomato_core::sql_completion;

pub(super) fn offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(index, _)| index)
}
fn accept(text: &str, range: CCursorRange, state: &mut State) -> Option<(String, CCursorRange)> {
    let candidate = state.matches.candidates.get(state.selected)?;
    let left = offset(text, state.matches.start);
    let right = offset(text, range.primary.index.into());
    if left > right {
        state.dismiss();
        return None;
    }
    if text.len() - (right - left) + candidate.text.len() > 8 * 1024 * 1024 {
        state.error = "SQL 正文超过 8 MiB，未插入补全项".into();
        return Some((text.to_owned(), range));
    }
    let mut result = String::with_capacity(text.len() - (right - left) + candidate.text.len());
    result.push_str(&text[..left]);
    result.push_str(&candidate.text);
    result.push_str(&text[right..]);
    let caret = state.matches.start + candidate.text.chars().count();
    state.dismiss();
    Some((result, CCursorRange::one(CCursor::new(caret))))
}

fn requests_completion(event: &Event) -> bool {
    match event {
        Event::Text(text) | Event::Ime(egui::ImeEvent::Commit(text)) => !text.is_empty(),
        Event::Key {
            key:
                Key::Backspace
                | Key::Delete
                | Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End,
            pressed: true,
            ..
        } => true,
        _ => false,
    }
}

fn refresh(state: &mut State, text: &str, range: CCursorRange) {
    if !range.is_empty() {
        state.dismiss();
        return;
    }
    state.requested = true;
    state.catalog_changed = false;
    let next = sql_completion::complete(text, range.primary.index.into(), state.catalog());
    if next != state.matches || !state.open {
        state.selected = 0;
        state.pressed = None;
        state.reveal = true;
    }
    state.open = !next.candidates.is_empty();
    state.matches = next;
}

fn handle_event(
    event: &Event,
    text: &str,
    range: CCursorRange,
    state: &mut State,
) -> Option<EventEdit> {
    let Event::Key {
        key,
        modifiers,
        pressed: true,
        ..
    } = event
    else {
        return None;
    };
    if *key == Key::S && modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd {
        state.save_requested = true;
        return Some(EventEdit::Selection(range));
    }
    if *key == Key::I && modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd {
        state.ai_requested = Some(false);
        return Some(EventEdit::Selection(range));
    }
    if *key == Key::D && modifiers.ctrl && !modifiers.shift && !modifiers.alt && !modifiers.mac_cmd
    {
        return Some(
            match tomato_core::sql_edit::duplicate(
                text,
                range.secondary.index.into(),
                range.primary.index.into(),
                range.primary.index.into(),
            ) {
                Ok(edit) => EventEdit::Replace(
                    edit.text,
                    CCursorRange::two(CCursor::new(edit.anchor), CCursor::new(edit.caret)),
                ),
                Err(error) => {
                    state.error = error.to_string();
                    EventEdit::Selection(range)
                }
            },
        );
    }
    if *key == Key::Escape {
        let was_open = state.open;
        state.dismiss();
        return was_open.then_some(EventEdit::Selection(range));
    }
    if state.open {
        let count = state.matches.candidates.len();
        match key {
            Key::ArrowDown => {
                state.selected = (state.selected + 1) % count;
                state.reveal = true;
            }
            Key::ArrowUp => {
                state.selected = if state.selected == 0 {
                    count - 1
                } else {
                    state.selected - 1
                };
                state.reveal = true;
            }
            Key::Enter | Key::Tab => {
                state.consumed_enter = *key == Key::Enter;
                return accept(text, range, state).map(EventEdit::from);
            }
            _ => return None,
        }
        return Some(EventEdit::Selection(range));
    }
    if *key == Key::Tab {
        // Original SQL Tab always inserts four spaces at the caret, also with Shift.
        let byte = offset(text, range.primary.index.into());
        if text.len() + 4 > 8 * 1024 * 1024 {
            state.error = "SQL 正文超过 8 MiB".into();
            return Some(EventEdit::Selection(range));
        }
        let mut result = text.to_owned();
        result.insert_str(byte, "    ");
        return Some(EventEdit::Replace(
            result,
            CCursorRange::one(CCursor::new(usize::from(range.primary.index) + 4)),
        ));
    }
    if *key == Key::Enter && modifiers.ctrl {
        state.consumed_enter = true;
        let sql = tomato_core::sql_edit::effective(
            text,
            range.secondary.index.into(),
            range.primary.index.into(),
        );
        if state.run_enabled && !sql.is_empty() {
            state.run_sql = Some(sql.to_owned());
        }
        return Some(EventEdit::Selection(range));
    }
    None
}
#[cfg(test)]
pub(super) fn render(ui: &mut Ui, text: &mut String, state: &mut State, tab: &str, enabled: bool) {
    render_config(ui, text, state, tab, enabled, None);
}
pub(super) fn render_in(
    ui: &mut Ui,
    text: &mut String,
    state: &mut State,
    tab: &str,
    enabled: bool,
    size: egui::Vec2,
) {
    render_config(ui, text, state, tab, enabled, Some(size));
}
fn render_config(
    ui: &mut Ui,
    text: &mut String,
    state: &mut State,
    tab: &str,
    enabled: bool,
    size: Option<egui::Vec2>,
) {
    let id = state
        .editor
        .prepare_id(ui.ctx(), ui.make_persistent_id(("sql-query-editor", tab)));
    let capture = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-sql-completion-fixture")))
        .unwrap_or(false);
    if capture || (ui.is_enabled() && std::mem::take(&mut state.focus_requested)) {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if capture
        && ui.ctx().data_mut(|data| {
            let sent = egui::Id::new("tomato-sql-completion-fixture-input");
            let first = data.get_temp::<bool>(sent).is_none();
            data.insert_temp(sent, true);
            first
        })
    {
        ui.input_mut(|input| input.events.push(Event::Text("Users.".into())));
    }
    state.consumed_enter = false;
    state.save_requested = false;
    state.run_sql = None;
    state.run_enabled = enabled;
    if let Some(event) = state.menu_event.take() {
        ui.input_mut(|input| input.events.push(event));
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let gesture = super::copy_drag::track(ui, id, text, state);
    if state.requested
        && (gesture.suppress_selection
            || !ui.is_enabled()
            || state
                .galley
                .as_ref()
                .is_some_and(|galley| galley.job.text != *text)
            || ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, Event::PointerButton { pos, pressed: true, .. }
                        if !state.popup_rect.is_some_and(|rect| rect.contains(*pos)))
                })
            }))
    {
        state.dismiss();
    }
    let mut keyboard_input = false;
    let mut refreshed_in_frame = false;
    let mut drop = gesture.drop;
    let mut edit_event = |event: &Event, text: &str, range: CCursorRange| {
        if let Event::PointerButton {
            pressed: false,
            pos,
            button: egui::PointerButton::Primary,
            ..
        } = event
            && let Some(drop) = drop.take_if(|drop| drop.pos == *pos)
        {
            return match drop.insert(text, range) {
                Ok(edit) => edit,
                Err(error) => {
                    state.error = error.into();
                    Some(EventEdit::Selection(range))
                }
            };
        }
        // Input can arrive in a single frame (for example Text followed by Enter).
        // Refresh before a key that navigates or accepts the popup so it sees the new prefix.
        if matches!(
            event,
            Event::Key {
                key: Key::ArrowUp | Key::ArrowDown | Key::Enter | Key::Tab,
                pressed: true,
                ..
            }
        ) && (keyboard_input || state.requested && state.catalog_changed)
        {
            refresh(state, text, range);
            keyboard_input = false;
            refreshed_in_frame = true;
        }
        if matches!(
            event,
            Event::Paste(_) | Event::Cut | Event::Ime(egui::ImeEvent::Preedit { .. })
        ) || matches!(event, Event::Key { key: Key::Z | Key::Y, modifiers, pressed: true, .. } if modifiers.command)
        {
            keyboard_input = false;
            state.dismiss();
        }
        let edit = handle_event(event, text, range, state);
        if matches!(&edit, Some(EventEdit::Replace(..)))
            || matches!(
                event,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            )
        {
            keyboard_input = false;
            state.dismiss();
        } else if edit.is_none() && requests_completion(event) {
            keyboard_input = true;
        }
        edit
    };
    let mut layouter = super::highlight::layout;
    let secondary = ui.input(|input| {
        input.pointer.secondary_down()
            || input.pointer.button_pressed(egui::PointerButton::Secondary)
            || input
                .pointer
                .button_released(egui::PointerButton::Secondary)
    });
    let mut field = egui::TextEdit::multiline(text)
        .id(id)
        .code_editor()
        .pointer_selection(!secondary && !gesture.suppress_selection)
        .desired_width(f32::INFINITY)
        .desired_rows(9)
        .layouter(&mut layouter)
        .event_editor(&mut edit_event, 32 * 1024 * 1024);
    if let Some(size) = size {
        field = field
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 40,
                right: 0,
                top: 0,
                bottom: 0,
            }))
            .min_size(size)
            .desired_width(size.x)
            .desired_rows(1);
    }
    let output = field.show(ui);
    let range = output.state.cursor.char_range().unwrap_or_default();
    state.changed = output.response.changed();
    state.selection = range;
    state.galley_pos = output.galley_pos;
    state.text_rect = Some(output.text_clip_rect.intersect(ui.clip_rect()));
    if state.changed {
        state.copy_drag = None;
    }
    super::copy_drag::paint(ui, id, state);
    if let Some(action) = super::editor_menu::show(&output.response, text, range, ui.is_enabled()) {
        use super::editor_menu::Action;
        match action {
            Action::Event(event) => {
                state.menu_event = Some(event);
                ui.ctx().request_repaint();
            }
            Action::SelectAll => {
                state.editor.select(ui.ctx(), 0..text.chars().count());
                state.selection =
                    CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
            }
            Action::Ai => state.ai_requested = Some(true),
            Action::Run if enabled => {
                let sorted = range.as_sorted_char_range();
                let sql =
                    tomato_core::sql_edit::effective(text, sorted.start.into(), sorted.end.into());
                if !sql.is_empty() {
                    state.run_sql = Some(sql.into());
                }
            }
            Action::Run => {}
        }
    }
    let pointer_in_popup = state.popup_rect.is_some_and(|rect| {
        ui.input(|input| {
            input
                .pointer
                .interact_pos()
                .is_some_and(|pos| rect.contains(pos))
        })
    });
    if gesture.suppress_selection
        || (!capture && !output.response.has_focus() && !pointer_in_popup)
        || !range.is_empty()
    {
        state.dismiss();
    } else {
        let changed = state.changed || state.caret != usize::from(range.primary.index);
        if keyboard_input && changed {
            refresh(state, text, range);
        } else if changed && !refreshed_in_frame {
            state.dismiss();
        } else if state.requested && state.catalog_changed {
            refresh(state, text, range);
        }
    }
    state.caret = range.primary.index.into();
    state.galley = Some(output.galley.clone());
    if state.open {
        let caret = output
            .galley
            .pos_from_cursor(range.primary)
            .translate(output.galley_pos.to_vec2());
        const ROW_HEIGHT: f32 = 26.;
        let count = state.matches.candidates.len();
        let visible = count.min(8);
        let height = visible as f32 * ROW_HEIGHT;
        let mut chosen = None;
        let popup = egui::Area::new(id.with("completions"))
            .order(egui::Order::Foreground)
            .fixed_pos(caret.left_bottom())
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1., Color32::from_gray(200)))
                    .inner_margin(0)
                    .show(ui, |ui| {
                        ui.set_width(298.);
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                        ui.spacing_mut().scroll.bar_width = 12.;
                        let mut scroll = egui::ScrollArea::vertical()
                            .max_height(height)
                            .min_scrolled_height(height)
                            .auto_shrink([false, false]);
                        if state.reveal {
                            let first = state
                                .selected
                                .saturating_sub(visible / 2)
                                .min(count - visible);
                            scroll = scroll.vertical_scroll_offset(first as f32 * ROW_HEIGHT);
                        }
                        scroll.show_rows(ui, ROW_HEIGHT, count, |ui, rows| {
                            for index in rows {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                                    egui::Sense::hover(),
                                );
                                let response = ui.interact(
                                    rect,
                                    id.with(("candidate", index)),
                                    egui::Sense::CLICK,
                                );
                                if response.is_pointer_button_down_on() {
                                    state.selected = index;
                                    state.pressed = Some(index);
                                }
                                let selected = state.selected == index;
                                if selected {
                                    ui.painter().rect_filled(rect, 0., Color32::from_gray(211));
                                } else if index % 2 == 1 {
                                    ui.painter().rect_filled(rect, 0., Color32::from_gray(248));
                                }
                                let candidate = &state.matches.candidates[index];
                                let color = Color32::from_gray(51);
                                let mut job = egui::text::LayoutJob::simple(
                                    format!("{}    {}", candidate.text, candidate.kind.label()),
                                    egui::FontId::monospace(12.),
                                    color,
                                    rect.width() - 12.,
                                );
                                job.wrap.max_rows = 1;
                                job.wrap.break_anywhere = true;
                                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                                ui.painter().galley(
                                    egui::pos2(
                                        rect.left() + 6.,
                                        rect.center().y - galley.size().y / 2.,
                                    ),
                                    galley,
                                    color,
                                );
                                if response.clicked()
                                    || (state.pressed == Some(index)
                                        && response.hovered()
                                        && ui.input(|input| {
                                            input
                                                .pointer
                                                .button_released(egui::PointerButton::Primary)
                                        }))
                                {
                                    chosen = Some(index);
                                }
                            }
                        });
                    });
            });
        state.popup_rect = Some(popup.response.rect);
        if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary)) {
            state.pressed = None;
        }
        state.reveal = false;
        if let Some(index) = chosen {
            state.selected = index;
            if let Some((value, new_range)) = accept(text, range, state) {
                let mut editor = output.state;
                let mut undo = editor.take_undoer();
                undo.add_undo_owned((range, text.clone()));
                *text = value;
                state.changed = true;
                editor.cursor.set_char_range(Some(new_range));
                undo.add_undo_owned((new_range, text.clone()));
                editor.set_undoer(undo);
                editor.store(ui.ctx(), id);
                ui.memory_mut(|memory| memory.request_focus(id));
                state.galley = None;
                ui.ctx().request_repaint();
            }
        }
    } else {
        state.popup_rect = None;
        state.pressed = None;
    }
    if capture
        && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
    {
        let trace = serde_json::json!({"liveCatalog":state.metadata.as_ref().is_some_and(|metadata|metadata.tables.iter().any(|table|table.name=="Users"&&table.columns==["id","name"])),"candidates":state.matches.candidates.iter().map(|candidate|&candidate.text).collect::<Vec<_>>(),"popupWidth":state.popup_rect.map(|rect|rect.width()),"popupHeight":state.popup_rect.map(|rect|rect.height()),"popupLeft":state.popup_rect.map(|rect|rect.left()),"popupTop":state.popup_rect.map(|rect|rect.top())});
        let _ = std::fs::write(path.with_extension("state.json"), trace.to_string());
    }
    state.editor.trim(ui.ctx());
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        text: &mut String,
        state: &mut State,
        events: Vec<Event>,
        time: f64,
    ) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 500.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let id = ui.make_persistent_id(("sql-query-editor", "test"));
                ui.memory_mut(|memory| memory.request_focus(id));
                render(ui, text, state, "test", true);
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
    fn click(point: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn state() -> State {
        State {
            metadata: Some(std::sync::Arc::new(sql_completion::Metadata {
                schema: "main".into(),
                database: "main".into(),
                namespaces: vec!["main".into()],
                tables: vec![sql_completion::Table {
                    name: "Users".into(),
                    columns: vec!["id".into(), "name".into()],
                }],
                ..Default::default()
            })),
            ..Default::default()
        }
    }
    #[test]
    fn mouse_caret_changes_dismiss_completion_and_catalog_arrival_does_not_reopen_it() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut text = "select".to_owned();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        assert!(!state.open, "opening existing SQL must not suggest");
        let at = |state: &State, char| {
            state
                .galley
                .as_ref()
                .unwrap()
                .pos_from_cursor(CCursor::new(char))
                .center()
                + state.galley_pos.to_vec2()
        };
        let inside = at(&state, 3);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::PointerMoved(inside), click(inside, true)],
            0.1,
        );
        frame(&ctx, &mut text, &mut state, vec![click(inside, false)], 0.2);
        assert_eq!(state.caret, 3);
        assert!(!state.open, "clicking a keyword must not suggest");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::ArrowRight, egui::Modifiers::NONE)],
            0.3,
        );
        assert!(state.open, "keyboard navigation may suggest");
        let same_caret = at(&state, 4);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::PointerMoved(same_caret), click(same_caret, true)],
            0.4,
        );
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![click(same_caret, false)],
            0.5,
        );
        assert_eq!(state.caret, 4);
        assert!(!state.open, "clicking the same caret must dismiss");
        state.catalog_changed = true;
        frame(&ctx, &mut text, &mut state, vec![], 0.6);
        assert!(
            !state.open,
            "async metadata must not undo a mouse dismissal"
        );
    }

    #[test]
    fn a_keyboard_request_can_pick_up_late_catalog_data() {
        let ctx = egui::Context::default();
        let mut editor = State::default();
        let mut text = String::new();
        frame(&ctx, &mut text, &mut editor, vec![], 0.);
        frame(
            &ctx,
            &mut text,
            &mut editor,
            vec![Event::Text("Users.".into())],
            0.1,
        );
        assert!(!editor.open);
        editor.metadata = state().metadata;
        editor.catalog_changed = true;
        frame(&ctx, &mut text, &mut editor, vec![], 0.2);
        assert!(editor.open);
        assert_eq!(editor.matches.candidates.len(), 2);
    }

    #[test]
    fn sql_editor_context_menu_matches_java_geometry_and_keeps_selected_sql() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut text = "SELECT 1;\nSELECT 2;".to_owned();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        let popup = ctx.memory(|memory| memory.focused()).unwrap().with("popup");
        state.editor.select(&ctx, 0..9);
        frame(&ctx, &mut text, &mut state, vec![], 0.1);
        let point = state.text_rect.unwrap().left_top() + egui::vec2(120., 20.);
        let event = |pressed| Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::PointerMoved(point), event(true)],
            0.2,
        );
        frame(&ctx, &mut text, &mut state, vec![event(false)], 0.3);
        let rect = ctx
            .memory(|memory| memory.area_rect(popup))
            .expect("right-click menu");
        assert_eq!(rect.min, point);
        assert_eq!(rect.size(), egui::vec2(84., 152.));
        let selected = state.selection.as_sorted_char_range();
        assert_eq!(usize::from(selected.start)..usize::from(selected.end), 0..9);
        assert!(!state.open);
    }
    #[test]
    fn real_editor_duplicates_unicode_selection_with_undo_and_runs_selected_sql_without_newline() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut text = "中文🍅\nnext".to_owned();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        let id = ctx.memory(|memory| memory.focused()).unwrap();
        let mut editor = egui::widgets::text_edit::TextEditState::load(&ctx, id).unwrap();
        editor
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(3))));
        editor.store(&ctx, id);
        let control = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        frame(&ctx, &mut text, &mut state, vec![key(Key::D, control)], 0.1);
        assert_eq!(text, "中文🍅中文🍅\nnext");
        assert_eq!(usize::from(state.selection.secondary.index), 3);
        assert_eq!(usize::from(state.selection.primary.index), 6);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Z, egui::Modifiers::COMMAND)],
            0.2,
        );
        assert_eq!(text, "中文🍅\nnext");
        text = "SELECT 1;\nCREATE TABLE hidden(id INTEGER)".into();
        state.editor.reset();
        state.invalidate();
        frame(&ctx, &mut text, &mut state, vec![], 0.3);
        let mut editor = egui::widgets::text_edit::TextEditState::load(&ctx, id).unwrap();
        editor
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(9))));
        editor.store(&ctx, id);
        state.open = false;
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Enter, control)],
            0.4,
        );
        assert_eq!(state.run_sql.as_deref(), Some("SELECT 1;"));
        assert_eq!(text, "SELECT 1;\nCREATE TABLE hidden(id INTEGER)");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Enter, control), Event::Text("replacement".into())],
            0.5,
        );
        assert_eq!(state.run_sql.as_deref(), Some("SELECT 1;"));
        assert!(text.starts_with("replacement\n"));
    }
    #[test]
    fn real_editor_completes_keywords_and_qualified_live_columns_with_keyboard_and_undo() {
        let ctx = egui::Context::default();
        let mut state = state();
        let mut text = String::new();
        frame(&ctx, &mut text, &mut state, vec![], 0.);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![Event::Text("sel".into())],
            1.,
        );
        assert!(state.open);
        assert_eq!(state.matches.candidates[0].text, "SELECT");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Enter, egui::Modifiers::COMMAND)],
            2.,
        );
        assert_eq!(text, "SELECT");
        assert!(state.consumed_enter);
        let id = ctx
            .memory(|memory| memory.focused())
            .expect("SQL editor focus");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Z, egui::Modifiers::COMMAND)],
            2.1,
        );
        assert_eq!(text, "sel");
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Y, egui::Modifiers::COMMAND)],
            2.2,
        );
        assert_eq!(text, "SELECT");
        // The source editor remains focused; a plain Tab inserts spaces after popup acceptance.
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(Key::Tab, egui::Modifiers::SHIFT)],
            3.,
        );
        assert_eq!(text, "SELECT    ");
        state.editor.release(&ctx);
        assert!(egui::widgets::text_edit::TextEditState::load(&ctx, id).is_none());
    }
    #[test]
    fn popup_arrow_wrap_and_mouse_click_accept_only_the_identifier_suffix() {
        for release_delay in [0.1, 1.0] {
            let ctx = egui::Context::default();
            let mut state = state();
            let mut text = String::new();
            frame(&ctx, &mut text, &mut state, vec![], 0.);
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![Event::Text("Users.".into())],
                0.1,
            );
            assert_eq!(state.matches.candidates.len(), 2);
            // Reproduce a layout cache eviction deterministically: same content,
            // different allocation. This must not reset the arrow-key selection.
            state.galley = Some(std::sync::Arc::new(
                (**state.galley.as_ref().unwrap()).clone(),
            ));
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![key(Key::ArrowUp, egui::Modifiers::NONE)],
                1.,
            );
            assert_eq!(state.selected, 1);
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![key(Key::ArrowDown, egui::Modifiers::NONE)],
                2.,
            );
            assert_eq!(state.selected, 0);
            let rect = state.popup_rect.expect("popup");
            assert!((rect.width() - 300.).abs() < 0.1);
            assert!((rect.height() - 54.).abs() < 0.1);
            let point = egui::pos2(rect.left() + 30., rect.top() + 1. + 26. + 13.);
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                3.,
            );
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                3. + release_delay,
            );
            assert_eq!(text, "Users.name");
        }
    }
}
