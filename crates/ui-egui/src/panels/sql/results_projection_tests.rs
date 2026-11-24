use super::results_tests::{fixture, frame, key, pointer};
use crate::tabs::query_results::{Cell, State};
use tomato_core::result_filter::Condition;
#[test]
fn filtered_reordered_result_keyboard_paste_and_copy_address_the_physical_row_and_field()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(crate::tabs::query_results::editing::Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan: tomato_core::query_edit::Plan {
                database_column_names: result.columns.clone(),
                editable_column_indexes: vec![1, 2, 3],
                primary_key_columns: vec!["中文字段".into()],
                uses_original_row_match: false,
                status_text: "可编辑".into(),
            },
            comments: Default::default(),
        },
    )));
    state.set_filter(&result, 3, Condition::parse(Some("=第 1999 行")))?;
    state.set_column_visible(0, false);
    state.reorder_columns(vec![3, 2, 1, 0])?;
    state.select(Cell { row: 0, column: 3 }, false, false, false);
    state.end_drag();
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("result-fixture")));
    let initial = frame(&ctx, &result, &mut state, 0., vec![]);
    assert!(
        !initial
            .text
            .iter()
            .any(|(text, _, _)| text == "missing" || text == "第 0 行")
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.1,
        vec![key(egui::Key::ArrowRight, false, false)],
    );
    assert_eq!(state.focus, Some(Cell { row: 0, column: 2 }));
    frame(
        &ctx,
        &result,
        &mut state,
        0.2,
        vec![egui::Event::Paste(
            "new literal\tnew empty\tignored hidden".into(),
        )],
    );
    let changes = state.editing.as_ref().unwrap().changes(&result, false)?;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].original[3].as_deref(), Some("第 1999 行"));
    assert_eq!(
        changes[0].modified.get(&2).and_then(|v| v.as_deref()),
        Some("new literal")
    );
    assert_eq!(
        changes[0].modified.get(&1).and_then(|v| v.as_deref()),
        Some("new empty")
    );
    assert!(!changes[0].modified.contains_key(&0));
    let output = frame(&ctx, &result, &mut state, 0.3, vec![egui::Event::Copy]);
    assert_eq!(output.copied, ["new literal\tnew empty"]);
    state.clear_filters();
    assert!(
        state
            .editing
            .as_ref()
            .unwrap()
            .cells
            .contains_key(&(1999, 2))
    );
    assert_eq!(result.value(1999, 2), Some("NULL"));
    Ok(())
}
#[test]
fn actual_header_drag_reorders_columns_and_click_sort_preserves_cell_identity() {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    let initial = frame(&ctx, &result, &mut state, 0., vec![]);
    let position = |name: &str| {
        let (_, point, _) = initial
            .text
            .iter()
            .find(|(text, _, _)| text == name)
            .unwrap();
        *point + egui::vec2(10., 5.)
    };
    let start = position("missing");
    let end = position("literal") + egui::vec2(20., 0.);
    frame(&ctx, &result, &mut state, 0.1, pointer(start, true, false));
    frame(
        &ctx,
        &result,
        &mut state,
        0.2,
        vec![egui::Event::PointerMoved(end)],
    );
    frame(&ctx, &result, &mut state, 0.3, pointer(end, false, false));
    assert_eq!(state.visible_columns().as_slice(), [1, 2, 0, 3]);
    assert!(state.view.sorts.is_empty());
    assert!(state.view.dragging.is_none());
}

#[test]
fn f2_replaces_selected_existing_text_and_returning_a_null_editor_to_empty_preserves_null()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(crate::tabs::query_results::editing::Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan: tomato_core::query_edit::Plan {
                database_column_names: result.columns.clone(),
                editable_column_indexes: vec![0, 3],
                primary_key_columns: vec!["中文字段".into()],
                uses_original_row_match: false,
                status_text: "可编辑".into(),
            },
            comments: Default::default(),
        },
    )));
    let id = egui::Id::new("result-fixture");
    state.select(Cell { row: 0, column: 3 }, false, false, false);
    state.end_drag();
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(&ctx, &result, &mut state, 0., vec![]);
    frame(
        &ctx,
        &result,
        &mut state,
        0.1,
        vec![key(egui::Key::F2, false, false)],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.2,
        vec![egui::Event::Text("替换".into())],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.3,
        vec![key(egui::Key::Enter, false, false)],
    );
    assert_eq!(state.display_value(&result, 0, 3), Some("替换"));
    state.select(Cell { row: 0, column: 0 }, false, false, false);
    state.end_drag();
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &result,
        &mut state,
        0.4,
        vec![key(egui::Key::F2, false, false)],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.5,
        vec![egui::Event::Text("x".into())],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.6,
        vec![key(egui::Key::Backspace, false, false)],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.7,
        vec![key(egui::Key::Enter, false, false)],
    );
    assert_eq!(state.display_value(&result, 0, 0), None);
    assert!(!state.editing.as_ref().unwrap().cells.contains_key(&(0, 0)));
    Ok(())
}
