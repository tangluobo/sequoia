use super::clipboard_shortcuts::{click, fixture, frame, frame_with_intent};
use super::*;
use egui::{Event, Key, Modifiers, Pos2};
fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    }
}
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    ctx
}
fn point(out: &egui::FullOutput, label: &str) -> Pos2 {
    out.shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + egui::vec2(3., text.galley.size().y / 2.))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}
fn select(ctx: &egui::Context, tab: &mut Tab, from: &str, to: &str, control: bool) {
    frame(ctx, tab, vec![]);
    let out = frame(ctx, tab, vec![]);
    click(ctx, tab, point(&out, from), Modifiers::default());
    click(
        ctx,
        tab,
        point(&out, to),
        Modifiers {
            shift: !control,
            ctrl: control,
            command: control,
            ..Default::default()
        },
    );
    frame(
        ctx,
        tab,
        vec![Event::ModifiersChanged(Modifiers::default())],
    );
}
fn value(tab: &Tab, row: usize, column: usize) -> Option<String> {
    let Panel::Sql {
        workbench, results, ..
    } = &tab.panel
    else {
        unreachable!()
    };
    if let Some(page) = &workbench.page {
        if let Some(value) = workbench
            .batch
            .as_ref()
            .and_then(|batch| batch.value(row, column))
        {
            return Some(value.into());
        }
        workbench
            .edits
            .get(&row)
            .unwrap_or(&page.rows[row])
            .get(column)
            .map(tomato_core::workbench::Cell::display)
    } else {
        workbench.query_results[0]
            .display_value(&results[0], row, column)
            .map(str::to_owned)
    }
}
#[test]
fn batch_edit_types_unicode_in_the_selected_cells_and_enter_stages_real_row_changes() {
    for (table, script) in [(true, false), (false, false), (false, true)] {
        let ctx = context();
        let mut tab = fixture(table, script);
        select(&ctx, &mut tab, "Alice", "Bob", false);
        let output = frame(
            &ctx,
            &mut tab,
            vec![Event::Text("AB中🙂".into()), key(Key::Backspace)],
        );
        assert!(
            tab.dirty(),
            "unconfirmed batch input must participate in close/refresh guards"
        );
        assert_eq!(value(&tab, 0, 1), Some("AB中".into()));
        assert_eq!(value(&tab, 1, 1), Some("AB中".into()));
        assert_eq!(value(&tab, 2, 1), Some("Charlie".into()));
        assert_eq!(output.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="AB中")).count(),2);
        let copied = frame(&ctx, &mut tab, vec![Event::Copy]);
        assert!(copied.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="AB中\nAB中")
        ));
        let Panel::Sql { workbench, .. } = &tab.panel else {
            unreachable!()
        };
        if table {
            assert!(workbench.edits.is_empty());
        } else {
            assert!(
                workbench.query_results[0]
                    .editing
                    .as_ref()
                    .unwrap()
                    .cells
                    .is_empty()
            );
        }
        frame(&ctx, &mut tab, vec![key(Key::Enter)]);
        let Panel::Sql {
            workbench, results, ..
        } = &tab.panel
        else {
            unreachable!()
        };
        if table {
            assert!(workbench.batch.is_none() && workbench.editor.is_none());
            let changes = workbench.changes();
            assert_eq!(changes.len(), 2);
            assert!(
                matches!(&changes[0],tomato_core::workbench::RowChange::Update{original,values} if original[1]==tomato_core::workbench::Cell::Text("Alice".into()) && values[1]==tomato_core::workbench::Cell::Text("AB中".into()))
            );
        } else {
            let editing = workbench.query_results[0].editing.as_ref().unwrap();
            assert!(editing.batch.is_none() && editing.editor.is_none());
            assert_eq!(editing.changes(&results[0], false).unwrap().len(), 2);
        }
    }
}
#[test]
fn batch_edit_escape_restores_existing_drafts_and_ctrl_selection_respects_physical_rows() {
    for (table, script) in [(true, false), (false, true)] {
        let ctx = context();
        let mut tab = fixture(table, script);
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        if table {
            workbench.table_view.projection.order = vec![2, 0, 1];
            let mut row = workbench.page.as_ref().unwrap().rows[0].clone();
            row[1] = tomato_core::workbench::Cell::Text("draft".into());
            workbench.edits.insert(0, row);
        } else {
            workbench.query_results[0].order = vec![2, 0, 1];
            workbench.query_results[0]
                .editing
                .as_mut()
                .unwrap()
                .cells
                .insert((0, 1), Some("draft".into()));
        }
        select(&ctx, &mut tab, "draft", "Charlie", true);
        frame(&ctx, &mut tab, vec![Event::Text("changed".into())]);
        assert_eq!(value(&tab, 0, 1), Some("changed".into()));
        assert_eq!(value(&tab, 2, 1), Some("changed".into()));
        assert_eq!(value(&tab, 1, 1), Some("Bob".into()));
        frame(&ctx, &mut tab, vec![key(Key::Escape)]);
        assert_eq!(value(&tab, 0, 1), Some("draft".into()));
        assert_eq!(value(&tab, 2, 1), Some("Charlie".into()));
        frame(
            &ctx,
            &mut tab,
            vec![Event::Text("new".into()), key(Key::Tab)],
        );
        assert_eq!(value(&tab, 0, 1), Some("new".into()));
        assert_eq!(value(&tab, 2, 1), Some("new".into()));
    }
}
#[test]
fn batch_edit_ime_preedit_is_replaced_once_and_enter_does_not_open_a_cell_editor() {
    for (table, script) in [(true, false), (false, true)] {
        let ctx = context();
        let mut tab = fixture(table, script);
        select(&ctx, &mut tab, "Alice", "Bob", false);
        let out = frame(&ctx, &mut tab, vec![]);
        assert!(out.platform_output.ime.is_some());
        frame(
            &ctx,
            &mut tab,
            vec![
                Event::Ime(egui::ImeEvent::Preedit {
                    text: "ni".into(),
                    active_range_chars: None,
                }),
                key(Key::Enter),
            ],
        );
        assert_eq!(value(&tab, 0, 1), Some("ni".into()));
        frame(
            &ctx,
            &mut tab,
            vec![
                Event::Ime(egui::ImeEvent::Commit("你".into())),
                Event::Text("好".into()),
                key(Key::Enter),
            ],
        );
        assert_eq!(value(&tab, 0, 1), Some("你好".into()));
        assert_eq!(value(&tab, 1, 1), Some("你好".into()));
        let Panel::Sql { workbench, .. } = &tab.panel else {
            unreachable!()
        };
        if table {
            assert!(workbench.editor.is_none() && workbench.batch.is_none());
        } else {
            let editing = workbench.query_results[0].editing.as_ref().unwrap();
            assert!(editing.editor.is_none() && editing.batch.is_none());
        }
    }
}
#[test]
fn batch_edit_validates_the_whole_selection_before_changing_typed_sqlite_cells() {
    let ctx = context();
    let mut tab = fixture(true, false);
    select(&ctx, &mut tab, "Alice", "2", true);
    frame(
        &ctx,
        &mut tab,
        vec![Event::Text("invalid integer".into()), key(Key::Enter)],
    );
    let Panel::Sql { workbench, .. } = &tab.panel else {
        unreachable!()
    };
    assert!(workbench.edits.is_empty() && workbench.batch.is_some());
    assert!(workbench.status.contains("批量输入无效"));
    frame(&ctx, &mut tab, vec![key(Key::Escape)]);
    assert_eq!(value(&tab, 0, 1), Some("Alice".into()));
    assert_eq!(value(&tab, 1, 0), Some("2".into()));
}
#[test]
fn batch_edit_readonly_columns_are_skipped_and_readonly_results_do_not_start_edits() {
    for (table, readonly) in [(true, false), (false, false), (true, true), (false, true)] {
        let ctx = context();
        let mut tab = fixture(table, false);
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        if table {
            workbench.definition.as_mut().unwrap().view = readonly;
            workbench.page.as_mut().unwrap().definition.columns[0].generated = true;
        } else if readonly {
            workbench.query_results[0].editing = None;
        } else {
            workbench.query_results[0]
                .editing
                .as_mut()
                .unwrap()
                .target
                .plan
                .editable_column_indexes = vec![1];
        }
        frame(&ctx, &mut tab, vec![]);
        let out = frame(&ctx, &mut tab, vec![]);
        click(
            &ctx,
            &mut tab,
            egui::pos2(7., point(&out, "Alice").y),
            Modifiers::default(),
        );
        click(
            &ctx,
            &mut tab,
            egui::pos2(7., point(&out, "Bob").y),
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        frame(
            &ctx,
            &mut tab,
            vec![Event::ModifiersChanged(Modifiers::default())],
        );
        frame(
            &ctx,
            &mut tab,
            vec![Event::Text("all".into()), key(Key::Enter)],
        );
        assert_eq!(value(&tab, 0, 0), Some("1".into()));
        assert_eq!(value(&tab, 1, 0), Some("2".into()));
        assert_eq!(
            value(&tab, 0, 1),
            Some(if readonly { "Alice" } else { "all" }.into())
        );
        assert_eq!(
            value(&tab, 1, 1),
            Some(if readonly { "Bob" } else { "all" }.into())
        );
    }
}

#[tokio::test]
async fn batch_edit_save_button_writes_only_selected_cells_to_an_owned_sqlite_database()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = context();
    let mut tab = fixture(true, false);
    let Panel::Sql { connection, .. } = &mut tab.panel else {
        unreachable!()
    };
    connection.set(
        "database",
        directory.path().join("batch.db").to_string_lossy().as_ref(),
    );
    let mut db = tomato_services::database::Databases::default();
    db.query(connection,"CREATE TABLE items(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO items VALUES(1,'Alice'),(2,'Bob'),(3,'Charlie');").await?;
    select(&ctx, &mut tab, "Alice", "Bob", false);
    frame(&ctx, &mut tab, vec![Event::Text("保存中文".into())]);
    let point = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("table-toolbar", "保存"))))
        .unwrap()
        .center();
    let intent = press_button(&ctx, &mut tab, point)
        .expect("save must dispatch the actual database command");
    let Intent::Command(command) = intent else {
        panic!("database command")
    };
    let Command::Database { connection, action } = *command else {
        panic!("table apply")
    };
    assert!(
        matches!(&action,tomato_core::workbench::DatabaseAction::Apply{changes,..} if changes.len()==2)
    );
    db.workbench(&connection, action).await?;
    let result = db
        .query(&connection, "SELECT name FROM items ORDER BY id")
        .await?;
    assert_eq!(
        result[0].rows,
        vec![vec!["保存中文"], vec!["保存中文"], vec!["Charlie"]]
    );
    Ok(())
}
fn press_button(ctx: &egui::Context, tab: &mut Tab, point: Pos2) -> Option<Intent> {
    let mut intent = None;
    for pressed in [true, false] {
        intent = frame_with_intent(
            ctx,
            tab,
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::default(),
                },
            ],
        )
        .1
        .or(intent);
    }
    intent
}
#[test]
fn batch_edit_query_save_button_finalizes_input_and_dispatches_the_original_row_predicates() {
    let ctx = context();
    let mut tab = fixture(false, true);
    select(&ctx, &mut tab, "Alice", "Bob", false);
    frame(&ctx, &mut tab, vec![Event::Text("writeback".into())]);
    let point = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("query-toolbar", "保存修改"))))
        .unwrap()
        .center();
    let Intent::Command(command) = press_button(&ctx, &mut tab, point).expect("query save") else {
        panic!("query writeback command")
    };
    let Command::SaveQueryRows {
        changes, result, ..
    } = *command
    else {
        panic!("query rows")
    };
    assert_eq!(result, 0);
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes[0].original,
        vec![Some("1".into()), Some("Alice".into())]
    );
    assert_eq!(
        changes[1].original,
        vec![Some("2".into()), Some("Bob".into())]
    );
    assert!(changes.iter().all(|change| change.modified.len() == 1
        && change.modified.get(&1) == Some(&Some("writeback".into()))
        && !change.delete));
}

#[test]
fn batch_edit_limits_input_before_allocating_per_cell_values_and_bounds_visible_text() {
    use crate::tabs::batch_edit::{Batch, Edit, LIMIT};
    assert!(Batch::new((0..1001).map(|row| (row, 0))).is_err());
    assert!(Batch::new((0..65_537).map(|column| (0, column))).is_err());
    let mut batch = Batch::new([(0, 0), (1, 0)].into_iter()).unwrap().unwrap();
    batch.edit(Edit::Text("中")).unwrap();
    assert!(batch.edit(Edit::Text(&"x".repeat(LIMIT))).is_err());
    assert_eq!(batch.text, "中");
    batch.edit(Edit::Text(&"中".repeat(1000))).unwrap();
    assert_eq!(batch.preview(1, 0).unwrap().chars().count(), 160);
    assert_eq!(batch.value(1, 0).unwrap().chars().count(), 1001);
}

#[test]
fn batch_edit_dragged_rectangle_commits_before_clicking_another_cell() {
    for (table, script) in [(true, false), (false, true)] {
        let ctx = context();
        let mut tab = fixture(table, script);
        frame(&ctx, &mut tab, vec![]);
        let out = frame(&ctx, &mut tab, vec![]);
        let first = point(&out, "Alice");
        let second = point(&out, "Bob");
        let third = point(&out, "Charlie");
        frame(
            &ctx,
            &mut tab,
            vec![
                Event::PointerMoved(first),
                Event::PointerButton {
                    pos: first,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ],
        );
        frame(&ctx, &mut tab, vec![Event::PointerMoved(second)]);
        frame(
            &ctx,
            &mut tab,
            vec![Event::PointerButton {
                pos: second,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            }],
        );
        frame(&ctx, &mut tab, vec![Event::Text("rectangle".into())]);
        assert_eq!(value(&tab, 0, 1), Some("rectangle".into()));
        assert_eq!(value(&tab, 1, 1), Some("rectangle".into()));
        click(&ctx, &mut tab, third, Modifiers::default());
        assert_eq!(value(&tab, 2, 1), Some("Charlie".into()));
        let Panel::Sql { workbench, .. } = &tab.panel else {
            unreachable!()
        };
        if table {
            assert!(workbench.batch.is_none());
        } else {
            assert!(
                workbench.query_results[0]
                    .editing
                    .as_ref()
                    .unwrap()
                    .batch
                    .is_none()
            );
        }
    }
}
