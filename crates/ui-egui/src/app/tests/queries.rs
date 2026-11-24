use super::*;
mod interaction;
mod scope;
mod toolbar;
use crate::app::query_tree::{self, Action};
use tomato_core::queries::{Entry, Scope};
fn edit(app: &mut TomatoApp, id: &str, text: &str) {
    let Panel::Sql { sql, workbench, .. } =
        &mut app.tabs.iter_mut().find(|tab| tab.id == id).unwrap().panel
    else {
        panic!("SQL")
    };
    *sql = text.into();
    workbench.document.as_mut().unwrap().changed(sql);
}
fn document<'a>(app: &'a TomatoApp, id: &str) -> (&'a str, &'a crate::tabs::queries::Document) {
    let Panel::Sql { sql, workbench, .. } =
        &app.tabs.iter().find(|tab| tab.id == id).unwrap().panel
    else {
        panic!("SQL")
    };
    (sql, workbench.document.as_ref().unwrap())
}
#[test]
fn legacy_query_tree_reads_saves_renames_and_deletes_real_files_without_changing_connections()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut c = Connection::new("SQLITE");
    c.name = "旧连接 中文".into();
    let store = Store {
        connections: vec![c.clone()],
        ..Default::default()
    };
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(config.clone());
    repository.load()?;
    repository.save(&store)?;
    let before = std::fs::read(&config)?;
    let scope = Scope {
        connection: c.name.clone(),
        database: "main".into(),
        path: String::new(),
    };
    let query = directory.path().join(scope.file("原查询")?);
    std::fs::create_dir_all(query.parent().unwrap())?;
    std::fs::write(&query, "SELECT '中文';\r\n")?;
    let mut app =
        TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(config.clone()));
    app.open_connection(c.clone());
    drain(&mut app)?;
    app.select_schema(&c.id, "main".into());
    drain(&mut app)?;
    app.query_action(Action::Expand {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
    });
    drain(&mut app)?;
    let tree = app.queries.tree(&c.id, "main").unwrap();
    assert_eq!(tree.directories[""].entries[0].name, "原查询");
    app.query_action(Action::Open {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
        name: "原查询".into(),
    });
    drain(&mut app)?;
    let id = app.active.clone().unwrap();
    assert_eq!(document(&app, &id).0, "SELECT '中文';\r\n");
    assert!(!document(&app, &id).1.dirty());
    app.query_action(Action::Open {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
        name: "原查询".into(),
    });
    assert_eq!(app.active.as_deref(), Some(id.as_str()));
    edit(&mut app, &id, "SELECT 2;");
    app.handle_intent(&id, Intent::SaveQuery);
    edit(&mut app, &id, "SELECT 3;");
    drain(&mut app)?;
    assert_eq!(std::fs::read_to_string(&query)?, "SELECT 2;");
    assert!(document(&app, &id).1.dirty());
    assert_eq!(
        std::fs::read_to_string(query.with_extension("sql.bak"))?,
        "SELECT '中文';\r\n"
    );
    app.handle_intent(&id, Intent::SaveQuery);
    drain(&mut app)?;
    assert!(!document(&app, &id).1.dirty());
    let renamed = tomato_services::queries::Repository::new(directory.path().into()).execute(
        tomato_core::queries::Request::Rename {
            scope: scope.clone(),
            entry: Entry {
                name: "原查询".into(),
                directory: false,
            },
            name: "新查询".into(),
        },
    )?;
    app.query_reply(
        query_tree::Job {
            connection: c.id.clone(),
            namespace: "main".into(),
            epoch: app.sql_hosts[&c.id].epoch.clone(),
            kind: query_tree::JobKind::Change {
                path: String::new(),
            },
        },
        Ok(Output::NamedQueries(renamed)),
    );
    drain(&mut app)?;
    assert_eq!(document(&app, &id).1.name.as_deref(), Some("新查询"));
    assert!(!query.exists());
    edit(&mut app, &id, "SELECT 4;");
    app.handle_intent(&id, Intent::SaveQuery);
    drain(&mut app)?;
    let renamed = directory.path().join(scope.file("新查询")?);
    assert_eq!(std::fs::read_to_string(&renamed)?, "SELECT 4;");
    let deleted = tomato_services::queries::Repository::new(directory.path().into()).execute(
        tomato_core::queries::Request::Delete {
            scope,
            entries: vec![Entry {
                name: "新查询".into(),
                directory: false,
            }],
        },
    )?;
    app.query_reply(
        query_tree::Job {
            connection: c.id.clone(),
            namespace: "main".into(),
            epoch: app.sql_hosts[&c.id].epoch.clone(),
            kind: query_tree::JobKind::Change {
                path: String::new(),
            },
        },
        Ok(Output::NamedQueries(deleted)),
    );
    drain(&mut app)?;
    assert!(!renamed.exists());
    assert!(app.tabs.iter().any(|tab| tab.id == id));
    assert_eq!(document(&app, &id).0, "SELECT 4;");
    assert!(std::fs::read(&config)? == before, "connection file changed");
    Ok(())
}
#[test]
fn named_query_external_edit_preserves_draft_and_new_queries_use_legacy_save_prompt()
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
    let c = Connection::new("SQLITE");
    app.store.connections.push(c.clone());
    app.open_connection(c.clone());
    drain(&mut app)?;
    app.select_schema(&c.id, "main".into());
    drain(&mut app)?;
    app.query_action(Action::New {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
    });
    let id = app.active.clone().unwrap();
    edit(&mut app, &id, "SELECT 9;");
    app.handle_intent(&id, Intent::SaveQuery);
    assert!(app.queries.dialog.is_some());
    app.queries.dialog = None;
    app.save_query_named(&id, "保存中文")?;
    drain(&mut app)?;
    let file = directory
        .path()
        .join(document(&app, &id).1.scope.file("保存中文")?);
    std::fs::write(&file, "SELECT 11;")?;
    edit(&mut app, &id, "SELECT 12;");
    app.handle_intent(&id, Intent::SaveQuery);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(std::time::Instant::now() < deadline, "timeout");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(std::fs::read_to_string(&file)?, "SELECT 11;");
    assert_eq!(document(&app, &id).0, "SELECT 12;");
    assert!(document(&app, &id).1.dirty());
    assert!(
        app.tabs
            .iter()
            .find(|tab| tab.id == id)
            .unwrap()
            .error
            .as_ref()
            .is_some_and(|error| error.contains("外部修改"))
    );
    Ok(())
}
