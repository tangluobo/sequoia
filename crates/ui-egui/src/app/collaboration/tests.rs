use super::*;

fn object() -> Arc<cloud::Object> {
    Arc::new(cloud::Object {
        id: "query".into(),
        project_id: "project".into(),
        parent_id: "connection".into(),
        kind: cloud::Kind::Query,
        name: "查询".into(),
        body: serde_json::json!({"text":"SELECT 1","database":"db"}),
        bytes: 8,
        revision: 1,
        deleted: false,
        modified: 0,
        modified_by: "user".into(),
        mutation_id: String::new(),
    })
}
#[test]
fn cloud_drafts_preserve_query_context_detect_metadata_edits_and_bound_saved_baselines()
-> anyhow::Result<()> {
    let mut source = (*object()).clone();
    source.body["text"] = "x".repeat(1024 * 1024).into();
    let source = Arc::new(source);
    let mut draft = Draft::from_object(&source);
    assert!(!draft.dirty());
    assert!(draft.baseline.len() < 140);
    assert!(draft.envelope.get("text").is_none());
    assert!(Arc::ptr_eq(draft.source.as_ref().unwrap(), &source));
    draft.name = "renamed".into();
    assert!(draft.dirty());
    let mutation = draft.mutation()?;
    assert_eq!(mutation.body["database"], "db");
    assert_eq!(mutation.body["text"].as_str().unwrap().len(), 1024 * 1024);
    assert_eq!(mutation.base_revision, 1);
    Ok(())
}
#[test]
fn original_query_save_routes_cloud_documents_and_rejects_view_only_projects() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let context = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &context,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    app.open_collaboration();
    let mut state = app.collaboration.take().unwrap();
    let mut view = cloud::View {
        selected: "project".into(),
        ..Default::default()
    };
    view.projects.push(cloud::Project {
        id: "project".into(),
        name: "p".into(),
        owner: "u".into(),
        role: cloud::Role::Owner,
        revision: 1,
        created: 0,
    });
    state.view = Arc::new(view);
    let mut c = Connection::new("SQLITE");
    c.set("database", ":memory:");
    let mut tab = tabs::create_tab(c, None)?;
    let Panel::Sql { sql, workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    *sql = "SELECT 2".into();
    let scope = tomato_core::queries::Scope {
        connection: "c".into(),
        database: "d".into(),
        path: String::new(),
    };
    let mut document = crate::tabs::queries::Document::new(scope, sql);
    document.loaded("查询".into(), tomato_services::queries::stamp(b"SELECT 1"));
    document.changed(sql);
    workbench.document = Some(document);
    let tab_id = tab.id.clone();
    state.queries.insert(
        tab_id.clone(),
        Query {
            object: (*object()).clone(),
            acknowledged: String::new(),
            mutation: String::new(),
        },
    );
    app.tabs.push(tab);
    app.collaboration = Some(state);
    app.request_query_save(&tab_id)?;
    let Panel::Sql { workbench, .. } = &app.tabs[0].panel else {
        unreachable!()
    };
    assert!(workbench.document.as_ref().unwrap().saving);
    assert!(app.queries.dialog.is_none());
    assert!(app.pending.is_empty());
    acknowledge(&mut app, &tab_id, false);
    app.request_query_save(&tab_id)?;
    acknowledge(&mut app, &tab_id, false);
    let state = app.collaboration.as_mut().unwrap();
    Arc::make_mut(&mut state.view).projects[0].role = cloud::Role::Viewer;
    assert!(app.request_query_save(&tab_id).is_err());
    Arc::make_mut(&mut app.collaboration.as_mut().unwrap().view).projects[0].role =
        cloud::Role::Owner;
    app.request_query_save(&tab_id)?;
    acknowledge(&mut app, &tab_id, true);
    assert!(
        app.tabs.is_empty(),
        "unchanged save must also finish a pending tab close"
    );
    Ok(())
}
fn acknowledge(app: &mut TomatoApp, tab: &str, close: bool) {
    let mut state = app.collaboration.take().unwrap();
    let mutation = state.queries[tab].mutation.clone();
    let view = Arc::make_mut(&mut state.view);
    view.accepted.push_back(mutation);
    view.saved
        .push_back(("query".into(), tomato_services::queries::stamp(b"SELECT 2")));
    if let Panel::Sql { workbench, .. } = &mut app.tabs[0].panel {
        workbench.document.as_mut().unwrap().close_after_save = close;
    }
    app.cloud_apply(&mut state);
    app.collaboration = Some(state);
    if !close {
        let Panel::Sql { workbench, .. } = &app.tabs[0].panel else {
            unreachable!()
        };
        let doc = workbench.document.as_ref().unwrap();
        assert!(!doc.saving);
        assert!(!doc.dirty());
    }
}
