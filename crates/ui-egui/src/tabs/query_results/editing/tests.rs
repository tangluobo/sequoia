use super::*;
fn fixture() -> (QueryResult, State) {
    let result = QueryResult {
        columns: vec!["id".into(), "name".into(), "expression".into()],
        rows: vec![
            vec!["2".into(), "b".into(), "42".into()],
            vec!["1".into(), "a".into(), "42".into()],
            vec!["3".into(), "c".into(), "42".into()],
        ],
        total_rows: 3,
        ..Default::default()
    };
    let plan = tomato_core::query_edit::Plan {
        database_column_names: vec!["id".into(), "name".into(), "\0query-result-column-2".into()],
        editable_column_indexes: vec![0, 1],
        primary_key_columns: vec!["id".into()],
        uses_original_row_match: false,
        status_text: "可编辑".into(),
    };
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan,
            comments: Default::default(),
        },
    )));
    (result, state)
}
#[test]
fn query_edits_follow_physical_rows_and_paste_starts_at_selection_minimum_skipping_expressions()
-> anyhow::Result<()> {
    let (result, mut state) = fixture();
    state.sort_by(&result, 0);
    assert_eq!(state.row(0), 1);
    state.select(Cell { row: 0, column: 1 }, false, false, false);
    state.select(Cell { row: 1, column: 2 }, true, false, false);
    assert_eq!(state.focus, Some(Cell { row: 1, column: 2 }));
    state.paste(
        &result,
        "first\tignored expression\r\nsecond\tignored\r\n\r\n",
    )?;
    let editing = state.editing.as_ref().unwrap();
    assert_eq!(editing.cells.len(), 2);
    assert_eq!(editing.value(&result, 1, 1), Some("first"));
    assert_eq!(editing.value(&result, 0, 1), Some("second"));
    assert_eq!(state.display_value(&result, 0, 2), Some("42"));
    assert_eq!(state.copy(&result)?.as_deref(), Some("first\nsecond"));
    assert_eq!(
        result.rows[0][1], "b",
        "the retained result is not duplicated or mutated before save succeeds"
    );
    state.editing.as_mut().unwrap().cancel();
    assert_eq!(state.display_value(&result, 1, 1), Some("a"));
    Ok(())
}
#[test]
fn query_edit_save_distinguishes_null_empty_and_literal_null_without_row_clones()
-> anyhow::Result<()> {
    let (mut result, mut state) = fixture();
    let editing = state.editing.as_mut().unwrap();
    editing.set(&result, 0, 1, None)?;
    editing.set(&result, 1, 1, Some(String::new()))?;
    editing.set(&result, 2, 1, Some("NULL".into()))?;
    assert_eq!(editing.changes(&result, false)?.len(), 3);
    editing.committed(&mut result, false);
    assert_eq!(result.value(0, 1), None);
    assert_eq!(result.value(1, 1), Some(""));
    assert_eq!(result.value(2, 1), Some("NULL"));
    assert!(!editing.dirty());
    editing.set(&result, 2, 1, Some("still pending".into()))?;
    editing.deleted.insert(0);
    let deleted = editing.changes(&result, true)?;
    assert_eq!(deleted.len(), 1);
    assert!(deleted[0].delete);
    assert!(deleted[0].modified.is_empty());
    editing.committed(&mut result, true);
    assert_eq!(result.total_rows, 2);
    assert_eq!(result.value(0, 1), Some(""));
    assert_eq!(result.value(1, 1), Some("NULL"));
    assert_eq!(editing.value(&result, 1, 1), Some("still pending"));
    assert_eq!(
        editing.cells.len(),
        1,
        "deleting one row must not save another row's edits"
    );
    Ok(())
}
#[test]
fn query_edit_budget_includes_sparse_map_entries_and_rejects_over_budget_paste_atomically()
-> anyhow::Result<()> {
    let (result, mut state) = fixture();
    state.select(Cell { row: 0, column: 1 }, false, false, false);
    state
        .editing
        .as_mut()
        .unwrap()
        .set(&result, 2, 1, Some("preserved".into()))?;
    assert!(state.paste(&result, &"x".repeat(8 * 1024 * 1024)).is_err());
    let editing = state.editing.as_ref().unwrap();
    assert_eq!(editing.cells.len(), 1);
    assert_eq!(editing.value(&result, 2, 1), Some("preserved"));
    assert!(editing.editor.is_none());
    Ok(())
}

#[test]
fn result_cell_switch_commits_the_previous_draft_and_budget_rejection_keeps_it()
-> anyhow::Result<()> {
    let (result, mut state) = fixture();
    state.start_edit(&result, Cell { row: 0, column: 1 });
    state
        .editing
        .as_mut()
        .unwrap()
        .editor
        .as_mut()
        .unwrap()
        .text = "previous draft".into();
    state.start_edit(&result, Cell { row: 1, column: 1 });
    assert_eq!(state.display_value(&result, 0, 1), Some("previous draft"));
    let editor = state.editing.as_mut().unwrap().editor.as_mut().unwrap();
    editor.text = "x".repeat(8 * 1024 * 1024);
    editor.changed = true;
    assert!(state.editing.as_mut().unwrap().finish(&result).is_err());
    assert_eq!(
        state
            .editing
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .text
            .len(),
        8 * 1024 * 1024
    );
    state
        .editing
        .as_mut()
        .unwrap()
        .editor
        .as_mut()
        .unwrap()
        .text = "within budget".into();
    state.editing.as_mut().unwrap().finish(&result)?;
    assert_eq!(state.display_value(&result, 1, 1), Some("within budget"));
    Ok(())
}
