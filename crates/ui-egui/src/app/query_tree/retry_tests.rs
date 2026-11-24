use super::*;
#[test]
fn resolver_failure_preserves_draft_disarms_close_and_refresh_retries_only_its_connection()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    // Any dispatched metadata retry can only reach this owned, inert listener.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
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
    c.set("host", "127.0.0.1");
    c.set("port", u64::from(listener.local_addr()?.port()));
    let host = crate::app::sql_tree::Host::new(c.clone());
    let epoch = host.epoch.clone();
    app.sql_hosts.insert(c.id.clone(), host);
    let mut tab = tabs::create_tab(c.clone(), None)?;
    let id = tab.id.clone();
    let Panel::Sql { sql, workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    *sql = "SELECT '未保存的内容';".into();
    workbench.schema = "public".into();
    let mut document = crate::tabs::queries::Document::new(
        Scope {
            connection: c.name.clone(),
            database: "fixture".into(),
            path: String::new(),
        },
        "",
    );
    document.close_after_save = true;
    document.changed(sql);
    workbench.document = Some(document);
    app.tabs.push(tab);
    app.queries.resolving.insert(c.id.clone(), epoch.clone());
    app.query_action(Action::Save(id.clone()));
    app.queries.deferred.push(Action::New {
        connection: "unrelated".into(),
        namespace: "other".into(),
        path: String::new(),
    });
    assert_eq!(app.queries.deferred.len(), 2);
    let job = || Job {
        connection: c.id.clone(),
        namespace: "public".into(),
        epoch: epoch.clone(),
        kind: JobKind::Resolve,
    };
    app.query_reply(job(), Err(anyhow::anyhow!("fixture metadata failure")));
    let Panel::Sql { sql, workbench, .. } = &app.tabs[0].panel else {
        unreachable!()
    };
    assert_eq!(sql, "SELECT '未保存的内容';");
    assert!(!workbench.document.as_ref().unwrap().close_after_save);
    assert!(app.queries.resolving.is_empty());
    assert_eq!(app.queries.deferred.len(), 1);
    assert!(app.queries.tree(&c.id, "public").unwrap().error.is_some());
    app.error = None;
    app.query_action(Action::Refresh {
        connection: c.id.clone(),
        namespace: "public".into(),
        path: String::new(),
    });
    assert_eq!(app.queries.resolving.get(&c.id), Some(&epoch));
    assert!(app.pending.values().any(|pending| matches!(
        &pending.purpose,
        Purpose::NamedQuery(Job {
            kind: JobKind::Resolve,
            ..
        })
    )));
    app.query_reply(
        job(),
        Ok(Output::Queries(vec![tomato_services::QueryResult {
            columns: vec!["current_database".into()],
            rows: vec![vec!["fixture".into()]],
            ..Default::default()
        }])),
    );
    assert!(app.queries.tree(&c.id, "public").unwrap().error.is_none());
    assert_eq!(app.queries.deferred.len(), 1);
    assert_eq!(
        app.query_action_connection(&app.queries.deferred[0]),
        Some("unrelated")
    );
    // Real filesystem replies may arrive later; errors must reach the user.
    app.query_reply(
        Job {
            connection: c.id,
            namespace: "public".into(),
            epoch,
            kind: JobKind::List {
                path: String::new(),
            },
        },
        Err(anyhow::anyhow!("fixture read denied")),
    );
    assert!(
        app.error
            .as_ref()
            .is_some_and(|text| text.contains("fixture read denied"))
    );
    Ok(())
}
