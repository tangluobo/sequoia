use super::*;
mod process;
fn fixture() -> anyhow::Result<(tempfile::TempDir, TomatoApp, egui::Context)> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("connections.json");
    let mut connection = Connection::new("ADB");
    connection.name = "原版 Android 设备".into();
    connection.set("adbExecutablePath", "  C:/Android 工具/adb.exe  ");
    connection.set("futureLegacy", serde_json::json!({"keep":42}));
    Store {
        connections: vec![connection],
        ..Default::default()
    }
    .save(&path)?;
    std::fs::write(
        root.path().join("global.json"),
        br#"{"adbExecutablePath":"C:/obsolete/global/adb.exe","future":{"keep":42}}"#,
    )?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(path));
    Ok((root, app, ctx))
}
#[test]
fn java_adb_fields_save_encrypted_and_invalid_port_keeps_draft() -> anyhow::Result<()> {
    let (root, mut app, ctx) = fixture()?;
    let path = root.path().join("connections.json");
    let before = std::fs::read(&path)?;
    let connection = app.store.connections[0].clone();
    let id = connection.id.clone();
    app.edit_connection(connection);
    for _ in 0..3 {
        let mut output = ctx.run_ui(Default::default(), |_| app.connection_dialog(&ctx));
        output.textures_delta.clear();
    }
    let mut output = ctx.run_ui(Default::default(), |_| app.connection_dialog(&ctx));
    output.textures_delta.clear();
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some(text.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect();
    for label in [
        "名称：",
        "主机：",
        "端口：",
        "无线配对",
        "独立窗口打开",
        "备注：",
    ] {
        assert!(labels.contains(&label), "Missing {label}: {labels:?}");
    }
    for label in [
        "用户名：",
        "密码：",
        "保存密码",
        "TLS端口：",
        "ADB程序：",
        "浏览...",
    ] {
        assert!(!labels.contains(&label));
    }
    app.dialog.as_mut().unwrap().adb_port = "not-a-port".into();
    app.apply_connection_draft();
    assert!(app.dialog.is_some());
    assert!(app.error.as_ref().unwrap().contains("端口"));
    assert_eq!(std::fs::read(&path)?, before);
    let dialog = app.dialog.as_mut().unwrap();
    dialog.adb_port = "6000".into();
    dialog.draft.set("host", "  adb://127.0.0.1  ");
    dialog.draft.set("fullscreen", true);
    app.apply_connection_draft();
    assert!(app.dialog.is_none());
    let saved = Store::load(&path)?.connections.remove(0);
    assert_eq!(saved.id, id);
    assert_eq!(saved.number("port"), 6000);
    assert_eq!(saved.text("host"), "adb://127.0.0.1");
    assert_eq!(
        saved.text("adbExecutablePath"),
        "  C:/Android 工具/adb.exe  "
    );
    assert!(saved.flag("fullscreen"));
    assert_eq!(saved.options["futureLegacy"]["keep"], 42);
    assert!(!saved.flag("usePassword") && !saved.flag("useKey") && !saved.flag("savePassword"));
    assert!(saved.options["username"].is_null() && saved.options["password"].is_null());
    assert!(std::fs::read_to_string(path)?.starts_with(tomato_core::legacy::MARKER));
    Ok(())
}
