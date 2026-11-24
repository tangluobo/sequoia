use super::*;
use std::collections::BTreeSet;
#[test]
fn toolbar_actions_match_original_rows_selection_focus_dirty_state_and_status() -> anyhow::Result<()>
{
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/table-design/original-actions.json"
    ))?;
    let ctx = egui::Context::default();
    for case in cases.as_array().unwrap() {
        let steps = case["steps"].as_array().unwrap();
        let mut draft = Draft::new("MYSQL", "sample_db".into())?;
        draft.request.columns = steps[0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| Column {
                name: row[0].as_str().unwrap().into(),
                data_type: "INT".into(),
                ..Default::default()
            })
            .collect();
        draft.flags = vec![Flags::default(); draft.request.columns.len()];
        draft.origins = vec![None; draft.request.columns.len()];
        draft.status = steps[0]["status"].as_str().unwrap().into();
        for expected in steps {
            let operation = expected["operation"].as_str().unwrap();
            if let Some(row) = operation.strip_prefix("select:") {
                draft.select(
                    Cell {
                        row: row.parse()?,
                        column: 0,
                    },
                    false,
                    false,
                );
            } else if let Some(json) = operation.strip_prefix("paste:") {
                draft.copied_fields = Some(json.into());
                draft.paste_fields(&ctx, "must not read an unrelated clipboard value")?;
            } else {
                match operation {
                    "initial" => {}
                    "all" => draft.select_all(),
                    command => draft.perform(
                        &ctx,
                        match command {
                            "add" => Action::Add,
                            "insert" => Action::Insert,
                            "delete" => Action::Delete,
                            "pk" => Action::PrimaryKey,
                            "up" => Action::Up,
                            "down" => Action::Down,
                            _ => unreachable!(),
                        },
                    ),
                }
            }
            let rows: Vec<Vec<String>> = draft
                .request
                .columns
                .iter()
                .enumerate()
                .map(|(row, column)| {
                    let flag =
                        |value: bool, index: usize| draft.flags[row].text(value, index).to_owned();
                    vec![
                        column.name.clone(),
                        column.data_type.clone(),
                        column.length.clone(),
                        column.scale.clone(),
                        flag(column.not_null, 0),
                        flag(column.primary_key, 1),
                        flag(column.auto_increment, 2),
                        column.default_value.clone(),
                        column.comment.clone(),
                        flag(column.unsigned, 3),
                        flag(column.zerofill, 4),
                        column.charset.clone(),
                        column.collation.clone(),
                        column.key_length.clone(),
                        flag(column.binary, 5),
                    ]
                })
                .collect();
            assert_eq!(serde_json::to_value(rows)?, expected["rows"], "{operation}");
            let selected: BTreeSet<_> = (0..draft.request.columns.len())
                .flat_map(|row| (0..8).map(move |column| (row, column)))
                .filter(|(row, column)| draft.selected(*row, *column))
                .collect();
            let expected_cells: BTreeSet<_> = expected["selected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|cell| {
                    (
                        cell[0].as_u64().unwrap() as usize,
                        cell[1].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            assert_eq!(selected, expected_cells, "{operation}");
            let focus = if expected["focusRow"].as_i64().unwrap() < 0 {
                None
            } else {
                Some(Cell {
                    row: expected["focusRow"].as_u64().unwrap() as usize,
                    column: expected["focusColumn"].as_u64().unwrap() as usize,
                })
            };
            assert_eq!(draft.focus, focus, "{operation}");
            assert_eq!(
                draft.dirty,
                expected["dirty"].as_bool().unwrap(),
                "{operation}"
            );
            assert_eq!(
                draft.status,
                expected["status"].as_str().unwrap(),
                "{operation}"
            );
            let status = draft.status.clone();
            let copied = draft.copy_fields()?.map(str::to_owned);
            assert_eq!(
                serde_json::to_value(copied)?,
                expected["copyJson"],
                "{operation}"
            );
            draft.status = status;
            draft.copied_fields = None;
        }
        draft.release(&ctx);
    }
    Ok(())
}
#[test]
fn cell_commits_infer_only_unset_types_and_escape_discards_while_navigation_commits()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut draft = Draft::new("MYSQL", "schema".into())?;
    draft.begin_edit(&ctx, Cell { row: 0, column: 0 });
    draft.edit.as_mut().unwrap().text = "amount".into();
    draft.begin_edit(&ctx, Cell { row: 0, column: 1 });
    assert_eq!(draft.request.columns[0].name, "amount");
    assert_eq!(draft.request.columns[0].data_type, "DECIMAL");
    assert_eq!(draft.request.columns[0].length, "18");
    assert_eq!(draft.request.columns[0].scale, "2");
    draft.edit.as_mut().unwrap().text = "VARCHAR".into();
    draft.cancel_edit(&ctx);
    assert_eq!(draft.request.columns[0].data_type, "DECIMAL");
    draft.begin_edit(&ctx, Cell { row: 0, column: 0 });
    draft.edit.as_mut().unwrap().text = "renamed".into();
    draft.perform(&ctx, Action::Add);
    assert_eq!(draft.request.columns[0].name, "renamed");
    assert_eq!(draft.request.columns.len(), 2);
    assert_eq!(draft.request.columns[0].data_type, "DECIMAL");
    draft.release(&ctx);
    Ok(())
}

#[test]
fn lazy_pages_keep_unvisited_options_out_of_sql_and_accept_late_metadata() -> anyhow::Result<()> {
    use tomato_core::table_design::Charset;
    let ctx = egui::Context::default();
    let mut draft = Draft::new("MYSQL", "sample_db".into())?;
    draft.set_field(Cell { row: 0, column: 0 }, "id".into());
    draft.request.options.engine = "should_not_leak".into();
    draft.request.comment = "unvisited".into();
    assert!(draft.snapshot().options.engine.is_empty());
    assert!(draft.snapshot().comment.is_empty());
    draft.switch_page(&ctx, Page::Options);
    assert!(!draft.options_loaded);
    let mut metadata = Metadata::empty(Dialect::Mysql);
    metadata.version = Some("8.4.11".into());
    metadata.engines = vec!["MyISAM".into(), "InnoDB".into()];
    metadata.charsets = vec![Charset {
        name: "utf8mb4".into(),
        collations: vec!["utf8mb4_0900_ai_ci".into(), "utf8mb4_bin".into()],
    }];
    draft.receive_metadata(Arc::new(metadata));
    assert!(draft.options_loaded);
    assert_eq!(draft.request.options.engine, "InnoDB");
    assert_eq!(draft.request.options.charset, "utf8mb4");
    assert_eq!(draft.request.options.collation, "utf8mb4_0900_ai_ci");
    draft.request.options.engine = "MyISAM".into();
    for page in [
        Page::Fields,
        Page::Indexes,
        Page::ForeignKeys,
        Page::Triggers,
        Page::Options,
        Page::Comment,
        Page::Sql,
    ] {
        draft.switch_page(&ctx, page);
    }
    assert_eq!(draft.request.options.engine, "MyISAM");
    assert_eq!(draft.snapshot().comment, "unvisited");
    draft.update_preview();
    assert!(draft.preview.contains("新表名"));
    assert!(draft.preview.contains("MyISAM"));
    assert!(draft.preview.contains("unvisited"));
    let allocation = draft.preview.as_ptr();
    draft.update_preview();
    assert_eq!(
        draft.preview.as_ptr(),
        allocation,
        "an unchanged preview must reuse its buffer"
    );
    draft.set_field(Cell { row: 0, column: 0 }, "another_id".into());
    draft.update_preview();
    assert!(draft.preview.contains("another_id"));
    draft.release(&ctx);
    Ok(())
}

#[test]
fn abandoning_editors_releases_only_their_own_undo_snapshots() -> anyhow::Result<()> {
    use egui::widgets::text_edit::TextEditState;
    let ctx = egui::Context::default();
    let mut draft = Draft::new("MYSQL", "sample_db".into())?;
    draft.begin_edit(&ctx, Cell { row: 0, column: 0 });
    let cell = egui::Id::new("designer-cell");
    let preview = egui::Id::new("designer-preview");
    let other = egui::Id::new("unrelated-query");
    draft.edit.as_mut().unwrap().editor.prepare_id(&ctx, cell);
    draft.preview_editor.prepare_id(&ctx, preview);
    for id in [cell, preview, other] {
        TextEditState::default().store(&ctx, id);
    }
    draft.cancel_edit(&ctx);
    assert!(TextEditState::load(&ctx, cell).is_none());
    assert!(TextEditState::load(&ctx, preview).is_some());
    draft.release(&ctx);
    assert!(TextEditState::load(&ctx, preview).is_none());
    assert!(TextEditState::load(&ctx, other).is_some());
    Ok(())
}

#[test]
fn clipboard_prefers_owned_payload_and_rejects_oversized_or_invalid_rows_atomically()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut draft = Draft::new("MYSQL", "sample_db".into())?;
    draft.set_field(Cell { row: 0, column: 0 }, "source".into());
    draft.select_row(0);
    let copied = draft.copy_fields()?.unwrap().to_owned();
    draft.paste_fields(&ctx, "unrelated clipboard text")?;
    assert_eq!(draft.request.columns.len(), 2);
    assert_eq!(draft.request.columns[1].name, "source");
    assert_eq!(draft.copied_fields.as_deref(), Some(copied.as_str()));
    draft.copied_fields = None;
    let previous = serde_json::to_value(draft.snapshot())?;
    for payload in [
        format!("[{}]", "{},".repeat(4096) + "{}"),
        " ".repeat(8 * 1024 * 1024 + 1),
        r#"[{"字段名":"valid"},{"字段名":[]} ]"#.into(),
    ] {
        assert!(draft.paste_fields(&ctx, &payload).is_err());
        assert_eq!(serde_json::to_value(draft.snapshot())?, previous);
    }
    draft.paste_fields(&ctx, r#"[{"字段名":"scalar", "默认值":1.00, "注释":true}]"#)?;
    let column = &draft.request.columns[2];
    assert_eq!(column.default_value, "1.00");
    assert_eq!(column.comment, "true");
    draft.release(&ctx);
    Ok(())
}
