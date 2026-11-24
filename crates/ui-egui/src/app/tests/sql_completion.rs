use super::*;
use tomato_core::workbench::DatabaseAction;

#[test]
fn new_query_types_in_blank_editor_and_executes_through_the_real_actor() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let connection = Connection::new("SQLITE");
    app.store.connections.push(connection.clone());
    app.open_connection(connection.clone());
    drain(&mut app)?;
    app.new_scoped_query(connection, "main".into());
    let id = app.active.clone().unwrap();
    let index = app.tabs.iter().position(|tab| tab.id == id).unwrap();
    assert!(matches!(&app.tabs[index].panel, Panel::Sql{sql,..} if sql.is_empty()));
    let frame = |app: &mut TomatoApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
    };
    frame(&mut app, vec![]);
    frame(
        &mut app,
        vec![egui::Event::Text("SELECT '中文' AS value;".into())],
    );
    let immediate =
        matches!(&app.tabs[index].panel, Panel::Sql{sql,..} if sql == "SELECT '中文' AS value;");
    let Panel::Sql { workbench, .. } = &app.tabs[index].panel else {
        unreachable!()
    };
    let point = workbench.completion.galley_pos + egui::vec2(400., 60.);
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(
        &mut app,
        vec![egui::Event::Text(" -- clicked blank space".into())],
    );
    let clicked = matches!(&app.tabs[index].panel, Panel::Sql{sql,workbench,..} if sql.ends_with(" -- clicked blank space") && workbench.document.as_ref().unwrap().dirty());
    assert!(
        immediate && clicked,
        "new query input: immediate={immediate}, clicked blank space={clicked}"
    );
    frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        }],
    );
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs[index].panel, Panel::Sql{results,..} if results.first().is_some_and(|result| result.value(0,0) == Some("中文")))
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}

#[test]
fn selected_sql_keyboard_intent_executes_only_selection_in_real_actor() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    let mut store = repository.load()?;
    let connection = Connection::new("SQLITE");
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let saved = std::fs::read(directory.path().join("connections.json"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.open_connection(connection.clone());
    drain(&mut app)?;
    let id = app.active.clone().unwrap();
    let source = "SELECT 7;\nCREATE TABLE should_not_exist(id INTEGER)";
    if let Panel::Sql { sql, workbench, .. } = &mut app.tabs[0].panel {
        *sql = source.into();
        workbench.completion.editor.reset();
        workbench.completion.invalidate();
    }
    let render = |app: &mut TomatoApp, events: Vec<egui::Event>| {
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
        let point = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == source
            {
                Some(text.pos + egui::vec2(2., 8.))
            } else {
                None
            }
        });
        output.textures_delta.clear();
        (intent, point)
    };
    let point = render(&mut app, vec![]).1.expect("actual SQL text");
    for pressed in [true, false] {
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let editor_id = ctx
        .memory(|memory| memory.focused())
        .expect("SQL editor must own focus");
    let mut state =
        egui::widgets::text_edit::TextEditState::load(&ctx, editor_id).expect("SQL text state");
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(9),
        )));
    state.store(&ctx, editor_id);
    let event = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    };
    let intent = render(&mut app, vec![event])
        .0
        .expect("selected SQL execution intent");
    match &intent {
        Intent::Command(command) => {
            assert!(
                matches!(command.as_ref(),Command::SqlScript{sql,mode:tomato_services::database::script::Mode::Run,..} if sql=="SELECT 7;")
            )
        }
        _ => panic!("SQL command expected"),
    }
    app.handle_intent(&id, intent);
    drain(&mut app)?;
    if let Panel::Sql { sql, results, .. } = &app.tabs[0].panel {
        assert_eq!(sql, source);
        assert_eq!(results[0].rows[0][0], "7");
    }
    app.submit(
        &id,
        Command::Sql {
            connection,
            sql: "SELECT COUNT(*) FROM sqlite_master WHERE name='should_not_exist'".into(),
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    if let Panel::Sql { results, .. } = &app.tabs[0].panel {
        assert_eq!(results[0].rows[0][0], "0");
    }
    anyhow::ensure!(
        std::fs::read(directory.path().join("connections.json"))? == saved,
        "selection execution changed configuration"
    );
    app.close(&id);
    Ok(())
}

#[test]
fn completion_runs_the_real_database_actor_without_blocking_or_discarding_foreground_queries()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    let mut store = repository.load()?;
    let connection = Connection::new("SQLITE");
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let saved = std::fs::read(directory.path().join("connections.json"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.open_connection(connection.clone());
    drain(&mut app)?;
    let id = app.active.clone().unwrap();
    app.submit(
        &id,
        Command::Sql {
            connection: connection.clone(),
            sql: "CREATE TABLE sample(id INTEGER, name TEXT)".into(),
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    let Panel::Sql { sql, workbench, .. } = &mut app.tabs[0].panel else {
        panic!("SQL")
    };
    sql.clear();
    workbench.completion.focus_requested = true;
    let frame = |app: &mut TomatoApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![egui::Event::Text("sample.".into())]);
    assert!(
        matches!(&app.tabs[0].panel, Panel::Sql { workbench, .. } if workbench.completion.requested)
    );
    app.submit(
        &id,
        Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::Completion {
                schema: "main".into(),
            },
        },
        Purpose::Normal,
    );
    assert!(!app.tabs[0].busy);
    app.submit(
        &id,
        Command::Sql {
            connection: connection.clone(),
            sql: "ALTER TABLE sample ADD COLUMN added TEXT; SELECT 'proof'".into(),
        },
        Purpose::Normal,
    );
    assert!(
        app.pending
            .values()
            .any(|pending| matches!(pending.purpose, Purpose::SqlCompletion(_)))
    );
    app.submit(
        &id,
        Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::Completion {
                schema: "main".into(),
            },
        },
        Purpose::Normal,
    );
    assert!(
        app.tabs[0].busy,
        "Metadata must not clear a foreground query's busy state"
    );
    assert_eq!(
        app.pending
            .values()
            .filter(|pending| matches!(pending.purpose, Purpose::SqlCompletion(_)))
            .count(),
        1
    );
    drain(&mut app)?;
    frame(&mut app, vec![]);
    let Panel::Sql {
        workbench, results, ..
    } = &app.tabs[0].panel
    else {
        panic!("SQL")
    };
    let metadata = workbench
        .completion
        .metadata
        .as_ref()
        .expect("live metadata");
    assert_eq!(metadata.tables[0].name, "sample");
    assert_eq!(metadata.tables[0].columns, ["id", "name", "added"]);
    assert!(workbench.completion.open);
    assert_eq!(
        workbench
            .completion
            .matches
            .candidates
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["added", "id", "name"]
    );
    assert!(results.iter().any(|result| {
        result
            .rows
            .iter()
            .any(|row| row.iter().any(|cell| cell == "proof"))
    }));
    app.submit(
        &id,
        Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::Completion {
                schema: "missing".into(),
            },
        },
        Purpose::Normal,
    );
    app.submit(
        &id,
        Command::Sql {
            connection,
            sql: "SELECT 42".into(),
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    let Panel::Sql {
        workbench, results, ..
    } = &app.tabs[0].panel
    else {
        panic!("SQL")
    };
    assert!(!workbench.completion.error.is_empty());
    assert!(app.tabs[0].error.is_none());
    assert!(results.iter().any(|result| {
        result
            .rows
            .iter()
            .any(|row| row.iter().any(|cell| cell == "42"))
    }));
    anyhow::ensure!(
        std::fs::read(directory.path().join("connections.json"))? == saved,
        "Read-only completion changed the legacy connection file"
    );
    app.close(&id);
    assert!(app.tabs.is_empty());
    Ok(())
}
