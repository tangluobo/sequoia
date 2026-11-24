use super::*;
use crate::app::tests::native_bridge::Bridge;

fn app(ctx: &egui::Context, path: &std::path::Path) -> TomatoApp {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    TomatoApp::with_repository(ctx, runtime, ConnectionFileRepository::new(path))
}
fn wait(app: &mut TomatoApp, ctx: &egui::Context) -> anyhow::Result<()> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !app.pending.is_empty() && std::time::Instant::now() < end {
        app.logic(ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    anyhow::ensure!(app.pending.is_empty(), "connection import worker timed out");
    Ok(())
}
fn key(ctx: &egui::Context, bridge: &Bridge, app: &mut TomatoApp, key: egui::Key) {
    bridge.frame(
        ctx,
        vec![egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        |ctx| app.import_dialog(ctx),
    );
    bridge.frame(ctx, vec![], |ctx| app.import_dialog(ctx));
}

#[test]
fn native_import_icon_does_not_select_and_double_click_label_folds_without_checking()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut app = app(&ctx, &dir.path().join("connections.json"));
    let plan = ImportPlan::parse(
        r#"[{"id":"g","name":"group"},{"id":"child","name":"child","parentId":"g","type":"SSH"}]"#,
        &app.store,
    )?;
    app.import_dialog = Some(Dialog::new(plan));
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    let group = bridge.text("group")?;
    bridge.click(&ctx, egui::pos2(68., group.y), |ctx| app.import_dialog(ctx));
    let dialog = app.import_dialog.as_ref().unwrap();
    assert_eq!(dialog.focused, None);
    assert_eq!(dialog.selected, BTreeSet::from([1]));
    // Separate gestures: a preceding icon click must not turn this double click
    // into egui's global 600 ms triple-click sequence.
    for _ in 0..7 {
        bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    }
    for pressed in [true, false, true, false] {
        bridge.frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(group),
                egui::Event::PointerButton {
                    pos: group,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            |ctx| app.import_dialog(ctx),
        );
    }
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    let dialog = app.import_dialog.as_ref().unwrap();
    assert!(dialog.collapsed.contains(&0));
    assert_eq!(dialog.focused, Some(0));
    assert_eq!(dialog.selected, BTreeSet::from([1]));
    assert!(bridge.text("child").is_err());
    Ok(())
}

#[test]
fn native_import_reads_selects_commits_encrypted_file_and_shows_success() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("connections.json");
    let mut store = Store::default();
    store.import(r#"[{"id":"g","name":"目录"},{"id":"s","name":"跳板","type":"SSH","password":"saved-fixture-only"}]"#)?;
    store.save(&target)?;
    let original = std::fs::read(&target)?;
    let source = directory.path().join("import.json");
    std::fs::write(
        &source,
        r#"[{"id":"g","name":"目录"},{"id":"s","name":"跳板","type":"SSH","password":"import-fixture-only"},{"id":"db","name":"数据库","parentId":"g","type":"MYSQL","sshTunnelHostId":"s","future":{"keep":42}},{"id":"external","name":"外部目录中的连接","type":"SSH","parentId":"unavailable"}]"#,
    )?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut app = app(&ctx, &target);
    app.read_connection_import(source.clone());
    assert!(app.root_modal_open());
    wait(&mut app, &ctx)?;
    assert_eq!(
        app.import_dialog.as_ref().unwrap().selected,
        BTreeSet::from([2, 3])
    );
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    bridge.text("外部目录中的连接")?;
    let clear = bridge.text("取消全选")?;
    bridge.click(&ctx, clear, |ctx| app.import_dialog(ctx));
    let submit = bridge.text("导入")?;
    bridge.click(&ctx, submit, |ctx| app.import_dialog(ctx));
    assert!(matches!(
        app.import_dialog.as_ref().unwrap().phase,
        Phase::Notice { .. }
    ));
    let ok = bridge.text("确定")?;
    bridge.click(&ctx, ok, |ctx| app.import_dialog(ctx));
    let defaults = bridge.text("仅选择未重复项")?;
    bridge.click(&ctx, defaults, |ctx| app.import_dialog(ctx));
    let submit = bridge.text("导入")?;
    bridge.click(&ctx, submit, |ctx| app.import_dialog(ctx));
    assert!(app.import_dialog.as_ref().unwrap().writing());
    app.apply_connection_import();
    assert_eq!(app.pending.len(), 1);
    wait(&mut app, &ctx)?;
    assert!(matches!(
        app.import_dialog.as_ref().unwrap().phase,
        Phase::Done(_)
    ));
    assert_eq!(app.store.connections.len(), 4);
    assert_eq!(
        app.store.connections[2].parent_id.as_deref(),
        Some(app.store.connections[0].id.as_str())
    );
    assert_eq!(
        app.store.connections[2].text("sshTunnelHostId"),
        app.store.connections[1].id
    );
    assert_eq!(app.store.connections[2].options["future"]["keep"], 42);
    assert!(app.store.connections[3].parent_id.is_none());
    assert_eq!(
        std::fs::read(
            app.repository
                .location()
                .unwrap()
                .with_file_name("connections.json.before-rust.bak")
        )?,
        original
    );
    assert!(!std::fs::read_to_string(&target)?.contains("fixture-only"));
    assert_eq!(Store::load(&target)?.connections.len(), 4);
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    let ok = bridge.text("确定")?;
    bridge.click(&ctx, ok, |ctx| app.import_dialog(ctx));
    assert!(app.import_dialog.is_none());
    app.store.connections[2].name = "保存后再次编辑".into();
    app.repository.save(&app.store)?;
    assert_eq!(Store::load(&target)?.connections[2].name, "保存后再次编辑");
    Ok(())
}

#[test]
fn native_import_virtual_navigation_icon_checks_and_folder_checks_match_java() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut app = app(&ctx, &directory.path().join("connections.json"));
    let mut nodes = vec![serde_json::json!({"id":"same","name":"目录"})];
    for i in 0..80 {
        nodes.push(serde_json::json!({"id":"duplicate","name":format!("项目 {i}"),"parentId":"same","type":"SSH"}));
    }
    nodes.push(serde_json::json!({"id":"other","name":"外部目录中的连接","parentId":"external","type":"SSH"}));
    app.import_dialog = Some(Dialog::new(ImportPlan::parse(
        &serde_json::to_string(&nodes)?,
        &app.store,
    )?));
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    let clear = bridge.text("取消全选")?;
    bridge.click(&ctx, clear, |ctx| app.import_dialog(ctx));
    let group = bridge.text("目录")?;
    bridge.click(&ctx, group, |ctx| app.import_dialog(ctx));
    key(&ctx, &bridge, &mut app, egui::Key::ArrowLeft);
    assert!(app.import_dialog.as_ref().unwrap().collapsed.contains(&0));
    key(&ctx, &bridge, &mut app, egui::Key::ArrowRight);
    key(&ctx, &bridge, &mut app, egui::Key::End);
    bridge.text("外部目录中的连接")?;
    key(&ctx, &bridge, &mut app, egui::Key::ArrowUp);
    let child = bridge.text("项目 79")?;
    key(&ctx, &bridge, &mut app, egui::Key::Space);
    assert!(app.import_dialog.as_ref().unwrap().selected.is_empty());
    bridge.click(&ctx, egui::pos2(80., child.y), |ctx| app.import_dialog(ctx));
    assert_eq!(
        app.import_dialog.as_ref().unwrap().selected,
        BTreeSet::from([80])
    );
    assert_eq!(app.import_dialog.as_ref().unwrap().focused, Some(80));
    key(&ctx, &bridge, &mut app, egui::Key::Home);
    let group = bridge.text("目录")?;
    bridge.click(&ctx, egui::pos2(68., group.y), |ctx| app.import_dialog(ctx));
    assert_eq!(
        app.import_dialog.as_ref().unwrap().selected,
        BTreeSet::from([0, 80]),
        "import group checks are independent"
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}

#[test]
fn changed_source_shows_modal_failure_and_read_cancel_discards_late_preview() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("import.json");
    std::fs::write(&source, r#"[{"id":"a","name":"connection","type":"SSH"}]"#)?;
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let target = directory.path().join("connections.json");
    let mut app = app(&ctx, &target);
    app.read_connection_import(source.clone());
    wait(&mut app, &ctx)?;
    std::fs::write(&source, "[]")?;
    app.apply_connection_import();
    wait(&mut app, &ctx)?;
    assert!(matches!(
        app.import_dialog.as_ref().unwrap().phase,
        Phase::Notice {
            kind: crate::native_dialog::message::Kind::Error,
            ..
        }
    ));
    assert!(app.error.is_none());
    assert!(!target.exists());
    bridge.frame(&ctx, vec![], |ctx| app.import_dialog(ctx));
    let ok = bridge.text("确定")?;
    bridge.click(&ctx, ok, |ctx| app.import_dialog(ctx));
    assert!(matches!(
        app.import_dialog.as_ref().unwrap().phase,
        Phase::Ready
    ));
    app.import_dialog = None;
    app.read_connection_import(source);
    app.import_reader = None;
    wait(&mut app, &ctx)?;
    assert!(app.import_dialog.is_none());
    Ok(())
}

#[test]
fn cancellation_after_commit_still_adopts_saved_models_and_revision() -> anyhow::Result<()> {
    use tomato_services::connections::{Request, execute};
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("import.json");
    std::fs::write(&source, r#"[{"id":"a","name":"connection","type":"SSH"}]"#)?;
    let ctx = egui::Context::default();
    let target = directory.path().join("connections.json");
    let mut app = app(&ctx, &target);
    let plan = Arc::new(ImportPlan::parse(
        &std::fs::read_to_string(&source)?,
        &app.store,
    )?);
    let output = execute(
        Request::Commit {
            source: source.clone(),
            source_signature: plan.source_signature(),
            guard: app.repository.save_guard()?.unwrap(),
            selected: BTreeSet::from([0]),
            prepared: Some(plan.clone()),
        },
        Default::default(),
    )?;
    let mut dialog = Dialog::from_shared(plan, Some(source));
    dialog.phase = Phase::Writing {
        id: 42,
        control: Default::default(),
        canceling: true,
    };
    app.import_dialog = Some(dialog);
    app.connection_import_reply(42, Ok(Output::ConnectionImport(output)));
    assert!(app.import_dialog.is_none());
    assert_eq!(app.store.connections.len(), 1);
    app.store.connections[0].name = "next edit".into();
    app.repository.save(&app.store)?;
    assert_eq!(Store::load(&target)?.connections[0].name, "next edit");
    Ok(())
}
