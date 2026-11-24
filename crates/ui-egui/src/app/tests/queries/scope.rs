use super::*;
#[test]
fn selected_postgres_editors_with_the_same_schema_keep_query_files_and_sidebar_refresh_separate()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let config = directory.path().join("connections.json");
    let mut app =
        TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(config.clone()));
    let mut base = Connection::new("POSTGRESQL");
    base.name = "隔离 PG".into();
    base.set("database", "base_db");
    let mut host = crate::app::sql_tree::Host::new(base.clone());
    host.schemas = std::sync::Arc::new(vec!["public".into()]);
    app.sql_hosts.insert(base.id.clone(), host);
    let mut editors = Vec::new();
    for (database, text) in [("database_a", "SELECT 'A';"), ("database_b", "SELECT 'B';")] {
        let mut connection = base.clone();
        connection.set("database", database);
        let mut tab = tabs::create_tab(connection.clone(), None)?;
        let Panel::Sql { sql, workbench, .. } = &mut tab.panel else {
            panic!("SQL editor")
        };
        *sql = text.into();
        workbench.schema = "public".into();
        workbench.selectors.selected = true;
        let id = tab.id.clone();
        app.tabs.push(tab);
        app.request_query_save(&id)?;
        app.queries.dialog = None;
        app.save_query_named(&id, "相同名称")?;
        assert!(
            app.pending
                .values()
                .all(|pending| matches!(pending.purpose, Purpose::NamedQuery(_))),
            "file routing must not require a database connection"
        );
        drain(&mut app)?;
        let scope = Scope {
            connection: base.name.clone(),
            database: database.into(),
            path: String::new(),
        };
        assert_eq!(
            std::fs::read_to_string(directory.path().join(scope.file("相同名称")?))?,
            text
        );
        let key = query_tree::identity::key(&connection, "public");
        let tree = app.queries.tree(&base.id, &key).unwrap();
        assert_eq!(tree.root.as_ref().unwrap().database, database);
        assert_eq!(tree.directories[""].entries[0].name, "相同名称");
        assert_eq!(document(&app, &id).1.namespace, key);
        assert!(
            matches!(&app.tabs.iter().find(|tab|tab.id==id).unwrap().panel,Panel::Sql{workbench,..} if workbench.schema=="public")
        );
        editors.push((id, key, scope));
    }
    assert_ne!(editors[0].1, editors[1].1);
    for (id, key, scope) in editors {
        app.query_action(Action::Open {
            connection: base.id.clone(),
            namespace: key,
            path: scope.path,
            name: "相同名称".into(),
        });
        assert_eq!(app.active.as_deref(), Some(id.as_str()));
    }
    assert!(app.error.is_none());
    assert!(app.pending.is_empty());
    assert!(!config.exists());
    Ok(())
}
#[test]
fn query_postgres_namespaces_use_actual_database_for_legacy_paths_and_reject_old_host_replies()
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
    let mut c = Connection::new("POSTGRESQL");
    c.name = "旧 PG 连接".into();
    let mut host = crate::app::sql_tree::Host::new(c.clone());
    host.schemas = std::sync::Arc::new(vec!["public".into(), "analytics".into()]);
    let epoch = host.epoch.clone();
    app.sql_hosts.insert(c.id.clone(), host);
    for namespace in ["public", "analytics"] {
        app.queries.trees.entry(c.id.clone()).or_default().insert(
            namespace.into(),
            query_tree::Tree {
                epoch: epoch.clone(),
                root: None,
                directories: Default::default(),
                error: None,
            },
        );
    }
    let scope = Scope {
        connection: c.name.clone(),
        database: "原数据库".into(),
        path: String::new(),
    };
    let file = directory.path().join(scope.file("跨 schema 的旧查询")?);
    std::fs::create_dir_all(file.parent().unwrap())?;
    std::fs::write(&file, "SELECT 1;")?;
    let reply = || {
        Ok(Output::Queries(vec![tomato_services::QueryResult {
            columns: vec!["current_database".into()],
            rows: vec![vec!["原数据库".into()]],
            ..Default::default()
        }]))
    };
    app.query_reply(
        query_tree::Job {
            connection: c.id.clone(),
            namespace: "public".into(),
            epoch: "old generation".into(),
            kind: query_tree::JobKind::Resolve,
        },
        reply(),
    );
    assert!(app.queries.tree(&c.id, "public").unwrap().root.is_none());
    assert!(app.pending.is_empty());
    app.query_reply(
        query_tree::Job {
            connection: c.id.clone(),
            namespace: "public".into(),
            epoch,
            kind: query_tree::JobKind::Resolve,
        },
        reply(),
    );
    drain(&mut app)?;
    for namespace in ["public", "analytics"] {
        let tree = app.queries.tree(&c.id, namespace).unwrap();
        assert_eq!(tree.root.as_ref().unwrap().database, "原数据库");
        assert_eq!(tree.directories[""].entries[0].name, "跨_schema_的旧查询");
    }
    // No PostgreSQL command was dispatched: this test feeds the metadata boundary
    // and uses only the real named-query filesystem actor afterwards.
    assert!(app.tabs.is_empty());
    assert!(app.error.is_none());
    Ok(())
}
#[test]
fn toolbar_connection_switch_saves_the_real_document_under_the_selected_connection()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut first = Connection::new("SQLITE");
    first.name = "连接 甲".into();
    let mut second = Connection::new("SQLITE");
    second.name = "连接 乙".into();
    let store = Store {
        connections: vec![first.clone(), second.clone()],
        ..Default::default()
    };
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    repository.load()?;
    repository.save(&store)?;
    let before = std::fs::read(directory.path().join("connections.json"))?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.open_connection(first.clone());
    drain(&mut app)?;
    app.query_action(Action::New {
        connection: first.id.clone(),
        namespace: "main".into(),
        path: String::new(),
    });
    let id = app.active.clone().unwrap();
    edit(&mut app, &id, "SELECT '切换后保存';");
    super::toolbar::choose_connection_through_toolbar(&mut app, &id, &second.name)?;
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==id).unwrap().panel,Panel::Sql {connection,workbench,..}if connection.text("database").is_empty()&&workbench.schema=="main")
    );
    app.request_query_save(&id)?;
    app.save_query_named(&id, "选择器查询")?;
    drain(&mut app)?;
    anyhow::ensure!(
        app.tabs
            .iter()
            .find(|tab| tab.id == id)
            .unwrap()
            .error
            .is_none(),
        "save failed: {:?}",
        app.tabs.iter().find(|tab| tab.id == id).unwrap().error
    );
    let destination = Scope {
        connection: second.name.clone(),
        database: "main".into(),
        path: String::new(),
    };
    assert_eq!(
        std::fs::read_to_string(directory.path().join(destination.file("选择器查询")?))?,
        "SELECT '切换后保存';"
    );
    assert!(
        !directory
            .path()
            .join(
                Scope {
                    connection: first.name,
                    database: "main".into(),
                    path: String::new()
                }
                .file("选择器查询")?
            )
            .exists()
    );
    let (_, document) = document(&app, &id);
    assert!(!document.dirty());
    assert_eq!(document.scope, destination);
    assert_eq!(
        std::fs::read(directory.path().join("connections.json"))?,
        before
    );
    Ok(())
}
