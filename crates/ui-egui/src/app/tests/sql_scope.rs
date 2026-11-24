use super::*;
#[test]
fn oracle_query_owner_selection_keeps_the_configured_service_and_legacy_file() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("connections.json");
    let repository = ConnectionFileRepository::new(file.clone());
    let mut store = repository.load()?;
    let mut oracle = Connection::new("ORACLE");
    oracle.name = "Oracle fixture".into();
    oracle.set("host", "127.0.0.1");
    oracle.set("database", "/service_name");
    oracle.set("username", "app");
    oracle.set("password", "");
    store.connections.push(oracle.clone());
    repository.save(&store)?;
    let original = std::fs::read(&file)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let tab = tabs::create_tab(oracle.clone(), None)?;
    let id = tab.id.clone();
    app.tabs.push(tab);
    app.active = Some(id.clone());
    app.choose_sql_connection(&id, &oracle.name);
    let task = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p.purpose, Purpose::SqlDatabases { .. }))
        .unwrap()
        .0;
    app.queue.deliver_for_test(
        task,
        Ok(Output::Database(
            tomato_core::workbench::DatabaseOutput::Databases(vec!["ANOTHER".into(), "APP".into()]),
        )),
    );
    app.logic(&ctx);
    let Panel::Sql {
        connection,
        workbench,
        ..
    } = &app.tabs[0].panel
    else {
        unreachable!()
    };
    assert_eq!(connection.text("database"), "/service_name");
    assert_eq!(workbench.schema, "APP");
    assert_eq!(
        tabs::workbench::query_database(connection, &workbench.schema),
        "APP"
    );
    assert_eq!(
        tabs::workbench::session_database(connection, &workbench.schema),
        "/service_name"
    );
    let routed = tabs::workbench::query_connection(connection, &workbench.schema);
    assert_eq!(routed.text("__tomatoOracleOwner"), "APP");
    app.choose_sql_database(&id, "ANOTHER");
    let Panel::Sql {
        connection,
        workbench,
        ..
    } = &app.tabs[0].panel
    else {
        unreachable!()
    };
    assert_eq!(connection.text("database"), "/service_name");
    assert_eq!(workbench.schema, "ANOTHER");
    assert_eq!(
        app.sql_hosts[&oracle.id].connection.text("database"),
        "/service_name"
    );
    assert_eq!(std::fs::read(file)?, original);
    Ok(())
}
#[test]
fn database_catalog_selection_keeps_actual_database_namespace_and_discards_old_replies()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    // Never drive this executor: only the real controller/queue boundary runs.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    let mut first = Connection::new("POSTGRESQL");
    first.name = "first".into();
    first.set("password", "");
    first.set("database", "shared");
    let mut second = Connection::new("POSTGRESQL");
    second.name = "second".into();
    second.set("password", "");
    second.set("database", "configured");
    app.store.connections = vec![first.clone(), second.clone()];
    let mut tab = tabs::create_tab(first.clone(), None)?;
    let id = tab.id.clone();
    if let Panel::Sql { workbench, .. } = &mut tab.panel {
        workbench.schema = "analytics".into();
        workbench.transaction = true;
        workbench.document = Some(tabs::queries::Document::new(
            tomato_core::queries::Scope {
                connection: first.name.clone(),
                database: "shared".into(),
                path: "nested".into(),
            },
            "SELECT 1",
        ));
    }
    app.tabs.push(tab);
    app.active = Some(id.clone());
    app.choose_sql_connection(&id, &second.name);
    let task = *app
        .pending
        .iter()
        .find(|(_, job)| matches!(job.purpose, Purpose::SqlDatabases { .. }))
        .unwrap()
        .0;
    app.queue.deliver_for_test(
        task,
        Ok(Output::Database(
            tomato_core::workbench::DatabaseOutput::Databases(vec![
                "another".into(),
                "shared".into(),
            ]),
        )),
    );
    app.logic(&ctx);
    let Panel::Sql {
        connection,
        workbench,
        ..
    } = &app.tabs[0].panel
    else {
        unreachable!()
    };
    assert_eq!(connection.id, second.id);
    assert_eq!(connection.text("database"), "shared");
    assert_eq!(workbench.schema, "analytics");
    assert!(!workbench.transaction);
    let document = workbench.document.as_ref().unwrap();
    assert_eq!(document.scope.database, "shared");
    assert_eq!(document.scope.path, "nested");
    assert_eq!(document.scope.connection, second.name);
    assert_eq!(
        workbench.selectors.databases.as_slice(),
        ["another", "shared"]
    );
    app.choose_sql_connection(&id, &first.name);
    let old = *app
        .pending
        .iter()
        .find(|(_, job)| matches!(job.purpose, Purpose::SqlDatabases { .. }))
        .unwrap()
        .0;
    app.choose_sql_connection(&id, &second.name);
    app.queue.deliver_for_test(
        old,
        Ok(Output::Database(
            tomato_core::workbench::DatabaseOutput::Databases(vec!["stale_database".into()]),
        )),
    );
    app.logic(&ctx);
    assert!(
        matches!(&app.tabs[0].panel,Panel::Sql {connection,workbench,..}if connection.id==second.id&&workbench.selectors.databases.is_empty())
    );
    assert!(!dir.path().join("connections.json").exists());
    Ok(())
}
#[test]
fn attached_sqlite_database_selection_keeps_the_file_session_transaction_and_actual_tables()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut connection = Connection::new("SQLITE");
    connection.set("database", ":memory:");
    app.store.connections.push(connection.clone());
    app.open_connection(connection.clone());
    drain(&mut app)?;
    let id = app.active.clone().unwrap();
    app.submit(&id,Command::Sql {connection:connection.clone(),sql:"ATTACH DATABASE ':memory:' AS other; CREATE TABLE other.marker(id INTEGER); INSERT INTO other.marker VALUES(7)".into()},Purpose::Normal);
    drain(&mut app)?;
    app.submit(
        &id,
        Command::Database {
            connection: connection.clone(),
            action: tomato_core::workbench::DatabaseAction::Transaction {
                action: tomato_core::workbench::TransactionAction::Begin,
            },
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    app.choose_sql_database(&id, "other");
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==id).unwrap().panel,Panel::Sql {connection:current,workbench,..}if current.text("database")==":memory:"&&workbench.schema=="other"&&workbench.transaction&&workbench.tables.iter().any(|table|table.table.schema=="other"&&table.table.name=="marker"))
    );
    app.submit(
        &id,
        Command::Sql {
            connection,
            sql: "SELECT id FROM other.marker".into(),
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==id).unwrap().panel,Panel::Sql {results,..}if results[0].value(0,0)==Some("7"))
    );
    Ok(())
}
