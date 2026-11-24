use super::*;
use crate::app::tests::native_bridge::Bridge;

#[test]
fn native_connection_export_controls_write_encrypted_selection_and_reimport_without_changing_source()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("connections.json");
    let connections = tomato_core::legacy::decode(
        r#"[{"id":"group","name":"测试目录"},{"id":"jump","parentId":"group","type":"SSH","name":"测试跳板","password":"fixture-only-password"},{"id":"db","parentId":"group","type":"MYSQL","name":"测试数据库","sshTunnelHostId":"jump","password":"database-only-password","future":{"keep":42}}]"#,
    )?;
    Store {
        connections,
        ..Default::default()
    }
    .save(&source)?;
    let original = std::fs::read(&source)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&source));
    app.export();
    bridge.frame(&ctx, vec![], |ctx| app.connection_export_dialog(ctx));
    assert!(!app.export_dialog.as_ref().unwrap().include_passwords);
    let export = bridge.text("导出")?;
    bridge.click(&ctx, export, |ctx| app.connection_export_dialog(ctx));
    assert!(matches!(
        app.export_dialog.as_ref().unwrap().phase,
        Phase::Warning(_)
    ));
    let ok = bridge.text("确定")?;
    bridge.click(&ctx, ok, |ctx| app.connection_export_dialog(ctx));
    assert!(matches!(
        app.export_dialog.as_ref().unwrap().phase,
        Phase::Ready
    ));
    let all = bridge.text("全选")?;
    bridge.click(&ctx, all, |ctx| app.connection_export_dialog(ctx));
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .selection
            .prepare(&app.store.connections, Credentials::Redacted)
            .is_ok()
    );
    let clear = bridge.text("取消全选")?;
    bridge.click(&ctx, clear, |ctx| app.connection_export_dialog(ctx));
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .selection
            .prepare(&app.store.connections, Credentials::Redacted)
            .is_err()
    );
    let row = bridge.text("测试跳板")?;
    bridge.click(&ctx, egui::pos2(62., row.y), |ctx| {
        app.connection_export_dialog(ctx)
    });
    let rows = app
        .export_dialog
        .as_ref()
        .unwrap()
        .selection
        .tree(&app.store.connections);
    assert_eq!(
        rows.rows[0].check,
        tomato_core::config::export::Check::Partial
    );
    assert_eq!(
        rows.rows[1].check,
        tomato_core::config::export::Check::Checked
    );
    let exported = directory.path().join("redacted.json");
    app.export_connections_to(exported.clone());
    app.export_connections_to(directory.path().join("duplicate-must-not-exist.json"));
    assert_eq!(app.pending.len(), 1);
    wait(&mut app, &ctx)?;
    assert!(
        !directory
            .path()
            .join("duplicate-must-not-exist.json")
            .exists()
    );
    assert!(matches!(
        app.export_dialog.as_ref().unwrap().phase,
        Phase::Done(_)
    ));
    let encoded = std::fs::read_to_string(&exported)?;
    assert!(encoded.starts_with(tomato_core::legacy::MARKER));
    let loaded = tomato_core::legacy::decode(&encoded)?;
    assert_eq!(loaded.len(), 2);
    assert!(loaded[1].text("password").is_empty());
    assert_eq!(std::fs::read(&source)?, original);
    app.export_dialog = None;
    app.export();
    bridge.frame(&ctx, vec![], |ctx| app.connection_export_dialog(ctx));
    let all = bridge.text("全选")?;
    bridge.click(&ctx, all, |ctx| app.connection_export_dialog(ctx));
    let include = bridge.text("包含密码")?;
    bridge.click(&ctx, include, |ctx| app.connection_export_dialog(ctx));
    assert!(app.export_dialog.as_ref().unwrap().include_passwords);
    let exported = directory.path().join("included.json");
    app.export_connections_to(exported.clone());
    wait(&mut app, &ctx)?;
    let content = std::fs::read_to_string(&exported)?;
    assert!(!content.contains("fixture-only-password"));
    let mut imported = Store::default();
    let plan = tomato_core::config::ImportPlan::parse(&content, &imported)?;
    plan.apply(&mut imported, &(0..plan.connections.len()).collect())?;
    let jump = imported
        .connections
        .iter()
        .find(|c| c.kind() == "SSH")
        .unwrap();
    let db = imported
        .connections
        .iter()
        .find(|c| c.kind() == "MYSQL")
        .unwrap();
    assert_eq!(jump.text("password"), "fixture-only-password");
    assert_eq!(db.text("password"), "database-only-password");
    assert_eq!(db.text("sshTunnelHostId"), jump.id);
    assert_eq!(db.options["future"]["keep"], 42);
    assert_eq!(std::fs::read(&source)?, original);
    app.export_dialog = None;
    app.export();
    app.export_dialog
        .as_mut()
        .unwrap()
        .selection
        .all(&app.store.connections, true);
    app.export_connections_to(source.clone());
    wait(&mut app, &ctx)?;
    assert!(
        matches!(&app.export_dialog.as_ref().unwrap().phase,Phase::Error(s) if s.contains("不能覆盖"))
    );
    assert_eq!(std::fs::read(&source)?, original);
    Ok(())
}
fn wait(app: &mut TomatoApp, ctx: &egui::Context) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !app.pending.is_empty() && std::time::Instant::now() < deadline {
        app.logic(ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    anyhow::ensure!(app.pending.is_empty(), "connection export actor timed out");
    Ok(())
}

#[test]
fn native_export_tree_keys_navigate_collapsed_and_virtual_rows_without_changing_checks()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut nodes = vec![serde_json::json!({"id":"group","name":"目录"})];
    for n in 0..80 {
        nodes.push(serde_json::json!({"id":format!("ssh-{n}"),"name":format!("连接 {n}"),"parentId":"group","type":"SSH"}));
    }
    nodes.push(serde_json::json!({"id":"other","name":"另一个目录"}));
    app.store.connections = tomato_core::legacy::decode(&serde_json::to_string(&nodes)?)?;
    app.export();
    bridge.frame(&ctx, vec![], |ctx| app.connection_export_dialog(ctx));
    let group = bridge.text("目录")?;
    bridge.click(&ctx, group, |ctx| app.connection_export_dialog(ctx));
    let key = |app: &mut TomatoApp, key| {
        bridge.frame(
            &ctx,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            |ctx| app.connection_export_dialog(ctx),
        );
        bridge.frame(&ctx, vec![], |ctx| app.connection_export_dialog(ctx));
    };
    key(&mut app, egui::Key::ArrowLeft);
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .collapsed
            .contains("group")
    );
    key(&mut app, egui::Key::ArrowRight);
    assert!(
        !app.export_dialog
            .as_ref()
            .unwrap()
            .collapsed
            .contains("group")
    );
    key(&mut app, egui::Key::ArrowDown);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("ssh-0")
    );
    key(&mut app, egui::Key::ArrowLeft);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("group")
    );
    key(&mut app, egui::Key::ArrowRight);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("ssh-0")
    );
    let group = bridge.text("目录")?;
    bridge.click(&ctx, egui::pos2(30., group.y), |ctx| {
        app.connection_export_dialog(ctx)
    });
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .collapsed
            .contains("group")
    );
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("group")
    );
    key(&mut app, egui::Key::ArrowRight);
    assert!(
        !app.export_dialog
            .as_ref()
            .unwrap()
            .collapsed
            .contains("group")
    );
    key(&mut app, egui::Key::End);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("other")
    );
    bridge.text("另一个目录")?;
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .selection
            .prepare(&app.store.connections, Credentials::Redacted)
            .is_err()
    );
    key(&mut app, egui::Key::ArrowUp);
    bridge.text("连接 79")?;
    key(&mut app, egui::Key::Space);
    assert!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .selection
            .prepare(&app.store.connections, Credentials::Redacted)
            .is_err()
    );
    let row = bridge.text("连接 79")?;
    bridge.click(&ctx, egui::pos2(80., row.y), |ctx| {
        app.connection_export_dialog(ctx)
    });
    let selected = app
        .export_dialog
        .as_ref()
        .unwrap()
        .selection
        .prepare(&app.store.connections, Credentials::Redacted)?;
    assert_eq!(
        selected
            .connections
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["group", "ssh-79"]
    );
    key(&mut app, egui::Key::Home);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("group")
    );
    bridge.text("目录")?;
    key(&mut app, egui::Key::PageDown);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("ssh-9")
    );
    key(&mut app, egui::Key::PageUp);
    assert_eq!(
        app.export_dialog.as_ref().unwrap().focused.as_deref(),
        Some("group")
    );
    let all = bridge.text("全选")?;
    bridge.click(&ctx, all, |ctx| app.connection_export_dialog(ctx));
    key(&mut app, egui::Key::Space);
    assert_eq!(
        app.export_dialog
            .as_ref()
            .unwrap()
            .selection
            .prepare(&app.store.connections, Credentials::Redacted)?
            .connections
            .len(),
        82
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
