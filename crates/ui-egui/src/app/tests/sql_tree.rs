use super::*;
#[test]
fn closing_and_reopening_sidebar_owner_preserves_other_editor_scope_and_live_tree()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let connection = Connection::new("SQLITE");
    app.store.connections.push(connection.clone());
    app.open_connection(connection.clone());
    drain(&mut app)?;
    let root = app.active.clone().unwrap();
    app.submit(&root,Command::Sql{connection:connection.clone(),sql:"CREATE TABLE main.first(id INTEGER); ATTACH DATABASE ':memory:' AS other; CREATE TABLE other.second(value TEXT)".into()},Purpose::Normal);
    drain(&mut app)?;
    app.submit(
        &root,
        Command::Database {
            connection: connection.clone(),
            action: tomato_core::workbench::DatabaseAction::Schemas,
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    let mut child = tabs::create_tab(
        connection.clone(),
        app.runtime.as_ref().map(|runtime| runtime.handle()),
    )?;
    child.source_id = "query-fixture-independent".into();
    let child_id = child.id.clone();
    if let Panel::Sql { workbench, .. } = &mut child.panel {
        workbench.schema = "main".into();
    }
    app.tabs.push(child);
    app.select_schema(&connection.id, "other".into());
    drain(&mut app)?;
    app.close(&root);
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==child_id).unwrap().panel,Panel::Sql{workbench,..} if workbench.schema=="main")
    );
    let rows = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "",
    );
    assert!(rows.iter().any(|row|matches!(&row.node,db_tree::Node::Table(_,table)if table.table.schema=="other"&&table.table.name=="second")));
    app.open_connection(connection.clone());
    let reopened = app.active.clone().unwrap();
    let Panel::Sql { workbench, .. } = &app
        .tabs
        .iter()
        .find(|tab| tab.id == reopened)
        .unwrap()
        .panel
    else {
        panic!("SQL")
    };
    assert_eq!(workbench.schema, "other");
    assert!(std::sync::Arc::ptr_eq(
        &workbench.schemas,
        &app.sql_hosts[&connection.id].schemas
    ));
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==child_id).unwrap().panel,Panel::Sql{workbench,..} if workbench.schema=="main")
    );
    drain(&mut app)?;
    app.close(&reopened);
    app.close(&child_id);
    assert!(!app.sql_hosts.contains_key(&connection.id));
    Ok(())
}
