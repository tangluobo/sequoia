use super::results_tests::{fixture, key, pointer};
use super::*;
use crate::tabs::query_results::{
    Cell, State, editing::Editing, temporal::Picker, temporal_model::Kind,
};
#[test]
fn result_calendar_month_day_time_apply_and_escape_use_the_real_popup_controls()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut result = fixture();
    result.rows[0][3] = "2024-02-29 12:34:56.75".into();
    result.column_types = vec![12, 12, 12, 93];
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan: tomato_core::query_edit::Plan {
                database_column_names: result.columns.clone(),
                editable_column_indexes: vec![3],
                primary_key_columns: vec!["中文字段".into()],
                uses_original_row_match: false,
                status_text: "可编辑".into(),
            },
            comments: Default::default(),
        },
    )));
    state.start_edit(&result, Cell { row: 0, column: 3 });
    let editor = state.editing.as_mut().unwrap().editor.as_mut().unwrap();
    editor.select_all = false;
    editor.temporal = Some(Box::new(Picker::new(
        Kind::DateTime,
        &editor.text,
        egui::pos2(50., 50.),
    )));
    let id = egui::Id::new("temporal-result-test");
    #[derive(Default)]
    struct Bridge {
        time: f64,
        events: Vec<egui::Event>,
        text: Vec<(String, egui::Pos2)>,
    }
    let bridge = std::rc::Rc::new(std::cell::RefCell::new(Bridge::default()));
    let native = bridge.clone();
    ctx.set_embed_viewports(false);
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let (time, events) = {
            let mut bridge = native.borrow_mut();
            (bridge.time, std::mem::take(&mut bridge.events))
        };
        let size = viewport.builder.inner_size.unwrap();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut input = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(rect),
            time: Some(time),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            viewport.ids.this,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                inner_rect: Some(rect),
                focused: Some(true),
                ..Default::default()
            },
        );
        let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
        native.borrow_mut().text = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some((text.galley.job.text.clone(), text.pos + egui::vec2(3., 5.)))
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
    });
    let popup = |state: &mut State, events| {
        let time = {
            let mut native = bridge.borrow_mut();
            native.time += 0.1;
            native.events = events;
            native.time
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 500.),
                )),
                time: Some(time),
                ..Default::default()
            },
            |ui| results::dialog(ui.ctx(), &result, state, id),
        );
        output.textures_delta.clear();
        bridge.borrow().text.clone()
    };
    popup(&mut state, vec![]);
    let first = popup(&mut state, vec![]);
    let next = first.iter().find(|(text, _)| text == "<").unwrap().1;
    for pressed in [true, false] {
        popup(&mut state, pointer(next, pressed, false));
    }
    let picker = state
        .editing
        .as_ref()
        .unwrap()
        .editor
        .as_ref()
        .unwrap()
        .temporal
        .as_ref()
        .unwrap();
    assert_eq!(picker.value.cursor.month, 1);
    assert_eq!(picker.value.selected.month, 2);
    let text = popup(&mut state, vec![]);
    let day = text.iter().find(|(text, _)| text == "15").unwrap().1;
    for pressed in [true, false] {
        popup(&mut state, pointer(day, pressed, false));
    }
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .selected
            .day,
        15
    );
    let time_y = state
        .editing
        .as_ref()
        .unwrap()
        .editor
        .as_ref()
        .unwrap()
        .temporal
        .as_ref()
        .unwrap()
        .height()
        - 72.;
    for pressed in [true, false] {
        popup(
            &mut state,
            pointer(egui::pos2(43., time_y + 15.), pressed, false),
        );
    }
    popup(
        &mut state,
        vec![
            key(egui::Key::A, true, false),
            egui::Event::Text("7".into()),
        ],
    );
    popup(&mut state, vec![key(egui::Key::ArrowUp, false, false)]);
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .time[0],
        8
    );
    popup(&mut state, vec![key(egui::Key::ArrowDown, false, false)]);
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .time[0],
        7
    );
    let height = state
        .editing
        .as_ref()
        .unwrap()
        .editor
        .as_ref()
        .unwrap()
        .temporal
        .as_ref()
        .unwrap()
        .height();
    let arrow = egui::pos2(71., height - 72. + 7.);
    popup(&mut state, pointer(arrow, true, false));
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .time[0],
        8
    );
    for _ in 0..2 {
        popup(&mut state, vec![]);
    }
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .time[0],
        8
    );
    for _ in 0..2 {
        popup(&mut state, vec![]);
    }
    popup(&mut state, pointer(arrow, false, false));
    assert!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .as_ref()
            .unwrap()
            .value
            .time[0]
            > 8
    );
    let time_y = state
        .editing
        .as_ref()
        .unwrap()
        .editor
        .as_ref()
        .unwrap()
        .temporal
        .as_ref()
        .unwrap()
        .height()
        - 72.;
    for pressed in [true, false] {
        popup(
            &mut state,
            pointer(egui::pos2(43., time_y + 15.), pressed, false),
        );
    }
    popup(
        &mut state,
        vec![
            key(egui::Key::A, true, false),
            egui::Event::Text("7".into()),
        ],
    );
    popup(&mut state, vec![key(egui::Key::Enter, false, false)]);
    assert!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .temporal
            .is_some(),
        "Enter in a spinner must commit its value without applying the popup"
    );
    let text = popup(&mut state, vec![]);
    let apply = text.iter().find(|(text, _)| text == "确定").unwrap().1;
    for pressed in [true, false] {
        popup(&mut state, pointer(apply, pressed, false));
    }
    let editor = state.editing.as_ref().unwrap().editor.as_ref().unwrap();
    assert!(editor.temporal.is_none());
    assert_eq!(editor.text, "2024-01-15 07:34:56");
    assert!(editor.changed);
    assert!(editor.focus);
    for index in 0..3usize {
        assert!(
            egui::widgets::text_edit::TextEditState::load(&ctx, id.with(("time-field", index)))
                .is_none()
        );
    }
    let editor = state.editing.as_mut().unwrap().editor.as_mut().unwrap();
    editor.temporal = Some(Box::new(Picker::new(
        Kind::DateTime,
        &editor.text,
        egui::pos2(50., 50.),
    )));
    popup(&mut state, vec![]);
    popup(&mut state, vec![key(egui::Key::Escape, false, false)]);
    let editing = state.editing.as_mut().unwrap();
    assert!(editing.editor.as_ref().unwrap().temporal.is_none());
    assert_eq!(editing.editor.as_ref().unwrap().text, "2024-01-15 07:34:56");
    editing.finish(&result)?;
    assert_eq!(
        editing
            .cells
            .get(&(0, 3))
            .and_then(|value| value.as_deref()),
        Some("2024-01-15 07:34:56")
    );
    assert_eq!(result.value(0, 3), Some("2024-02-29 12:34:56.75"));
    ctx.set_embed_viewports(true);
    let connection = tomato_core::Connection::new("MYSQL");
    let mut saved = None;
    for events in [
        vec![],
        pointer(egui::pos2(45., 416.), true, false),
        pointer(egui::pos2(45., 416.), false, false),
    ] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 450.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                saved = super::query_edit::render(
                    ui,
                    &result,
                    &mut state,
                    super::query_edit::Context {
                        connection: &connection,
                        schema: "public",
                        index: 0,
                        busy: false,
                        id: egui::Id::new("temporal-save-footer"),
                    },
                )
                .or(saved.take());
            },
        );
        output.textures_delta.clear();
    }
    let Some(Intent::Command(command)) = saved else {
        panic!("actual footer Save must produce a write command");
    };
    let Command::SaveQueryRows {
        connection,
        changes,
        result: index,
        ..
    } = *command
    else {
        panic!("wrong footer action");
    };
    assert_eq!(connection.text("database"), "public");
    assert_eq!(index, 0);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].original[3].as_deref(),
        Some("2024-02-29 12:34:56.75")
    );
    assert_eq!(
        changes[0]
            .modified
            .get(&3)
            .and_then(|value| value.as_deref()),
        Some("2024-01-15 07:34:56")
    );
    assert_eq!(changes[0].modified.len(), 1);
    assert!(!changes[0].delete);
    Ok(())
}
