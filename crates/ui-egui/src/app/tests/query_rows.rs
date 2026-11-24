use super::*;
use anyhow::Context;
#[test]
fn result_save_keeps_the_execution_connection_when_toolbar_selection_changes() -> anyhow::Result<()>
{
    use tomato_services::database::script::{Mode, Script, Statement};
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    // Never drive this executor: exercise actual delivery and controls without opening sockets.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut source = Connection::new("MYSQL");
    source.set("database", "original_db");
    let mut tab = crate::tabs::create_tab(source.clone(), None)?;
    let id = tab.id.clone();
    if let Panel::Sql { workbench, .. } = &mut tab.panel {
        workbench.schema = "original_db".into();
    }
    app.tabs.push(tab);
    app.active = Some(id.clone());
    let sql = "SELECT id,value FROM items";
    app.submit(
        &id,
        Command::SqlScript {
            connection: source.clone(),
            sql: sql.into(),
            mode: Mode::Run,
        },
        Purpose::Normal,
    );
    let task = *app.pending.keys().next().unwrap();
    let mut replacement = Connection::new("POSTGRESQL");
    replacement.set("database", "another_db");
    if let Panel::Sql {
        connection,
        workbench,
        ..
    } = &mut app.tabs[0].panel
    {
        *connection = replacement;
        workbench.schema = "another_namespace".into();
    }
    let editing = tomato_services::database::query_rows::Editing {
        table: tomato_core::workbench::TableId {
            schema: "original_db".into(),
            name: "items".into(),
        },
        plan: tomato_core::query_edit::Plan {
            database_column_names: vec!["id".into(), "value".into()],
            editable_column_indexes: vec![1],
            primary_key_columns: vec!["id".into()],
            uses_original_row_match: false,
            status_text: "可编辑".into(),
        },
        comments: Default::default(),
    };
    let result = tomato_services::QueryResult {
        columns: vec!["id".into(), "value".into()],
        rows: vec![vec!["1".into(), "original".into()]],
        ..Default::default()
    };
    app.queue.deliver_for_test(
        task,
        Ok(Output::Script(Box::new(Script {
            source: sql.into(),
            statements: vec![Statement {
                range: 0..sql.len(),
                select: true,
                source_table: Some("items".into()),
                elapsed_ms: 0,
                success: true,
                error: None,
                result: Some(0),
                has_result_set: true,
                row_count: 1,
                affected: None,
                truncated: false,
                editing: Some(editing),
                edit_error: None,
            }],
            results: vec![result],
            success_count: 1,
            ..Default::default()
        }))),
    );
    app.logic(&ctx);
    let render = |app: &mut TomatoApp, events| {
        let mut intent = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                intent = crate::panels::render(ui, &mut app.tabs[0], &mut app.icons);
            },
        );
        let save = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Mesh(mesh) = &shape.shape {
                    let rect = mesh.calc_bounds();
                    ((14.0..=16.0).contains(&rect.width())
                        && (10.0..=12.0).contains(&rect.height()))
                    .then_some(rect.center())
                } else {
                    None
                }
            })
            .max_by(|a, b| a.y.total_cmp(&b.y));
        output.textures_delta.clear();
        (intent, save)
    };
    render(&mut app, vec![]);
    if let Panel::Sql {
        results, workbench, ..
    } = &mut app.tabs[0].panel
    {
        let scope = workbench.result_context.as_ref().unwrap();
        assert_eq!(scope.connection.id, source.id);
        assert_eq!(scope.namespace, "original_db");
        workbench.query_results[0].editing.as_mut().unwrap().set(
            &results[0],
            0,
            1,
            Some("edited".into()),
        )?;
    }
    let point = render(&mut app, vec![])
        .1
        .context("original result footer Save button")?;
    let mut submitted = None;
    for pressed in [true, false] {
        let (intent, _) = render(
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
        submitted = intent.or(submitted);
    }
    assert!(
        matches!(submitted,Some(Intent::Command(command)) if matches!(command.as_ref(),Command::SaveQueryRows{connection,result:0,changes,..}if connection.id==source.id&&connection.kind()=="MYSQL"&&connection.text("database")=="original_db"&&changes.len()==1))
    );
    Ok(())
}
#[test]
fn query_save_reply_survives_unrelated_reads_and_cannot_modify_a_replaced_result()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    // This executor is intentionally never driven: only actual completion
    // delivery/controller transitions run; native writes have separate fixtures.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let connection = Connection::new("SQLITE");
    let mut tab = crate::tabs::create_tab(connection.clone(), None)?;
    let id = tab.id.clone();
    let editing = tomato_services::database::query_rows::Editing {
        table: tomato_core::workbench::TableId {
            schema: "main".into(),
            name: "items".into(),
        },
        plan: tomato_core::query_edit::Plan {
            database_column_names: vec!["id".into(), "value".into()],
            editable_column_indexes: vec![0, 1],
            primary_key_columns: vec!["id".into()],
            uses_original_row_match: false,
            status_text: "可编辑".into(),
        },
        comments: Default::default(),
    };
    if let Panel::Sql {
        results, workbench, ..
    } = &mut tab.panel
    {
        workbench.results_generation = 4;
        results.push(tomato_services::QueryResult {
            columns: vec!["id".into(), "value".into()],
            rows: vec![vec!["1".into(), "old".into()]],
            ..Default::default()
        });
        let mut state = crate::tabs::query_results::State::default();
        state.initialize(&results[0]);
        let mut pending = crate::tabs::query_results::editing::Editing::new(editing.clone());
        pending.set(&results[0], 0, 1, Some("saved".into()))?;
        state.editing = Some(Box::new(pending));
        workbench.query_results.push(state);
    }
    app.tabs.push(tab);
    app.active = Some(id.clone());
    let changes = if let Panel::Sql {
        results, workbench, ..
    } = &app.tabs[0].panel
    {
        workbench.query_results[0]
            .editing
            .as_ref()
            .unwrap()
            .changes(&results[0], false)?
    } else {
        unreachable!()
    };
    app.submit(
        &id,
        Command::SaveQueryRows {
            connection: connection.clone(),
            result: 0,
            editing,
            changes,
        },
        Purpose::Normal,
    );
    let saved = *app
        .pending
        .iter()
        .find(|(_, job)| {
            matches!(
                job.purpose,
                Purpose::QuerySave {
                    result: 0,
                    generation: 4
                }
            )
        })
        .unwrap()
        .0;
    app.submit(
        &id,
        Command::ReadText {
            path: directory.path().join("unused.sql"),
        },
        Purpose::SqlRead,
    );
    assert!(
        app.pending.contains_key(&saved),
        "a read must not discard an in-flight mutation reply"
    );
    let read = *app
        .pending
        .iter()
        .find(|(_, job)| matches!(job.purpose, Purpose::SqlRead))
        .unwrap()
        .0;
    app.queue
        .deliver_for_test(read, Ok(Output::Text("SELECT 'new SQL'".into())));
    app.logic(&ctx);
    assert!(
        app.tabs[0].busy,
        "the pending write still owns the busy state"
    );
    assert!(
        matches!(&app.tabs[0].panel,Panel::Sql{workbench,..} if workbench.query_results[0].saving)
    );
    app.submit(
        &id,
        Command::Sql {
            connection,
            sql: "SELECT 'replacement'".into(),
        },
        Purpose::Normal,
    );
    let read = *app
        .pending
        .iter()
        .find(|(_, job)| matches!(job.purpose, Purpose::SqlRequest(_)))
        .unwrap()
        .0;
    app.queue.deliver_for_test(
        read,
        Ok(Output::Queries(vec![tomato_services::QueryResult {
            columns: vec!["replacement".into()],
            rows: vec![vec!["new record".into()]],
            ..Default::default()
        }])),
    );
    app.logic(&ctx);
    assert!(app.tabs[0].busy);
    assert!(app.pending.contains_key(&saved));
    app.queue.deliver_for_test(
        saved,
        Ok(Output::SavedQueryRows {
            result: 0,
            affected: 1,
            deleting: false,
        }),
    );
    app.logic(&ctx);
    assert!(!app.tabs[0].busy);
    assert!(
        matches!(&app.tabs[0].panel,Panel::Sql{results,workbench,..} if results[0].value(0,0)==Some("new record") && workbench.results_generation==5 && workbench.query_results.is_empty())
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
