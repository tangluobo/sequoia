use super::*;

#[test]
fn copied_connections_keep_order_credentials_and_parent_and_inline_names_survive_save_conflicts()
-> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut group = Connection::new("SSH");
    group.kind = None;
    group.name = "目录".into();
    let mut connection = Connection::new("SSH");
    connection.name = "主机".into();
    connection.parent_id = Some(group.id.clone());
    connection.set("password", "fixture-only");
    connection.set("future", serde_json::json!({"nested":[1,2,3]}));
    let other = Connection::new("MYSQL");
    Store {
        connections: vec![group.clone(), connection.clone(), other.clone()],
        ..Default::default()
    }
    .save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
    app.copy_connection(&connection.id);
    let saved = Store::load(&path)?;
    assert_eq!(saved.connections[2].name, "主机 - 副本");
    assert_ne!(saved.connections[2].id, connection.id);
    assert_eq!(saved.connections[2].parent_id, connection.parent_id);
    assert_eq!(saved.connections[2].options, connection.options);
    assert_eq!(saved.connections[3].id, other.id);
    app.collapsed.remove(&group.id);
    app.rename_connection(&connection.id);
    assert!(render_frame(&ctx, &mut app));
    app.connection_rename.as_mut().unwrap().name = " 新名称 ".into();
    app.submit_connection_name();
    assert!(app.connection_rename.is_none());
    assert_eq!(Store::load(&path)?.connections[1].name, "新名称");
    assert_eq!(
        Store::load(&path)?.connections[1].options,
        connection.options
    );
    app.new_folder_at(Some(group.id.clone()));
    assert_eq!(
        app.dialog.as_ref().unwrap().draft.parent_id.as_deref(),
        Some(group.id.as_str())
    );
    assert!(app.dialog.as_ref().unwrap().draft.kind.is_none());
    app.dialog = None;
    app.new_connection_at(Some(group.id.clone()));
    assert_eq!(app.picker_parent.as_deref(), Some(group.id.as_str()));
    app.new_connection_at(None);
    assert!(app.picker_parent.is_none());
    app.picker = None;
    std::fs::write(&path, b"external update")?;
    app.copy_connection(&connection.id);
    assert_eq!(app.store.connections.len(), 4);
    app.rename_connection(&group.id);
    app.connection_rename.as_mut().unwrap().name = "失败草稿".into();
    app.submit_connection_name();
    assert_eq!(app.store.connections[0].name, "目录");
    assert_eq!(app.connection_rename.as_ref().unwrap().name, "失败草稿");
    assert_eq!(std::fs::read(&path)?, b"external update");
    Ok(())
}

#[test]
fn failed_connection_edit_import_and_delete_keep_model_drafts_and_external_file()
-> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut group = Connection::new("SSH");
    group.kind = None;
    group.name = "分组".into();
    let mut child = Connection::new("SSH");
    child.parent_id = Some(group.id.clone());
    child.name = "子项".into();
    Store {
        connections: vec![group.clone(), child.clone()],
        ..Default::default()
    }
    .save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &egui::Context::default(),
        runtime,
        ConnectionFileRepository::new(&path),
    );
    let original = std::fs::read(&path)?;
    std::fs::write(&path, b"external update")?;
    app.edit_connection(child.clone());
    app.dialog.as_mut().unwrap().draft.name = "尚未保存".into();
    app.apply_connection_draft();
    assert_eq!(app.store.connections[1].name, "子项");
    assert_eq!(app.dialog.as_ref().unwrap().draft.name, "尚未保存");
    assert!(!app.delete_connections(std::slice::from_ref(&group.id), false));
    assert_eq!(app.store.connections.len(), 2);
    app.import_dialog = Some(import::Dialog::new(tomato_core::config::ImportPlan::parse(
        r#"[{"id":"new","name":"新导入","type":"SSH"}]"#,
        &app.store,
    )?));
    app.apply_connection_import();
    assert!(app.import_dialog.is_some());
    assert_eq!(app.store.connections.len(), 2);
    assert_eq!(std::fs::read(&path)?, b"external update");
    std::fs::write(&path, original)?;
    app.apply_connection_import();
    assert!(app.import_dialog.is_none());
    assert_eq!(Store::load(&path)?.connections.len(), 3);
    app.connection_selection
        .extend([group.id.clone(), child.id.clone()]);
    app.request_connection_delete(&child.id);
    assert!(matches!(&app.confirmation,Some(Confirmation::Delete(ids)) if ids.len()==2));
    assert!(app.delete_connections(std::slice::from_ref(&group.id), true));
    let saved = Store::load(&path)?;
    assert_eq!(saved.connections.len(), 2);
    assert!(
        saved
            .connections
            .iter()
            .find(|c| c.id == child.id)
            .unwrap()
            .parent_id
            .is_none()
    );
    assert!(app.delete_connections(std::slice::from_ref(&child.id), false));
    assert_eq!(Store::load(&path)?.connections.len(), 1);
    Ok(())
}
