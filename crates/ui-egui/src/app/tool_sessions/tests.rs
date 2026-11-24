use super::*;
use std::time::{Duration, Instant};
fn connection() -> Connection {
    let mut c = Connection::new("TOOL");
    c.name = "原版下载器".into();
    c.set("toolType", "downloader");
    c.set("future", serde_json::json!({"keep":42}));
    c
}
fn state(tab: &Tab) -> &crate::tabs::downloader::State {
    let Panel::Tool(tool) = &tab.panel else {
        panic!("Expected downloader tool")
    };
    tool.downloader.as_ref().expect("Initialized downloader")
}
fn frame(app: &mut TomatoApp) {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="下载器初始化失败")));
}
fn until(app: &mut TomatoApp, condition: impl Fn(&TomatoApp) -> bool) {
    let start = Instant::now();
    loop {
        frame(app);
        for tab in &app.tabs {
            assert!(state(tab).error.is_none(), "{:?}", state(tab).error);
        }
        if condition(app) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Downloader did not become ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn saved_tool_factory_initializes_downloader_without_starting_a_worker() -> anyhow::Result<()> {
    let tab = crate::tabs::create_tab(connection(), None)?;
    assert!(state(&tab).manager.is_none());
    assert!(!state(&tab).show_header);
    assert!(ToolState::new("downloader").downloader.is_some());
    assert!(ToolState::new("json_tool").downloader.is_none());
    Ok(())
}
#[test]
fn downloader_saved_and_catalog_views_share_history_settings_actor_and_release_the_last_owner()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("connections.json");
    let saved = connection();
    Store {
        connections: vec![saved.clone()],
        ..Default::default()
    }
    .save(&config)?;
    let before = std::fs::read(&config)?;
    let history = root.path().join("downloads.json");
    std::fs::write(&history,br#"{"version":1,"tasks":[{"url":"http://127.0.0.1:9/legacy.bin","fileName":"legacy.bin","state":"CANCELLED","futureRow":{"keep":42}}],"future":{"keep":43}}"#)?;
    let history_before = std::fs::read(&history)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&config));
    app.open_connection(saved.clone());
    until(&mut app, |app| state(&app.tabs[0]).loaded_settings);
    assert_eq!(state(&app.tabs[0]).path, history);
    assert_eq!(state(&app.tabs[0]).rows[0].file_name, "legacy.bin");
    assert_eq!(std::fs::read(&history)?, history_before);
    let tab_id = app.tabs[0].id.clone();
    app.open_connection(saved.clone());
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active.as_deref(), Some(tab_id.as_str()));
    app.open_tool("downloader", "下载器");
    until(&mut app, |app| state(&app.tabs[1]).loaded_settings);
    assert_eq!(app.tabs.len(), 2);
    assert!(Arc::ptr_eq(
        state(&app.tabs[0]).manager.as_ref().unwrap(),
        state(&app.tabs[1]).manager.as_ref().unwrap()
    ));
    assert!(!state(&app.tabs[0]).show_header && state(&app.tabs[1]).show_header);
    state(&app.tabs[0]).manager.as_ref().unwrap().send(
        tomato_services::downloader::Action::Configure(tomato_services::downloader::Settings {
            directory: root.path().display().to_string(),
            threads: 3,
            concurrent: 1,
        }),
    )?;
    until(&mut app, |app| state(&app.tabs[1]).defaults.threads == 3);
    app.active = Some(app.tabs[0].id.clone());
    until(&mut app, |app| state(&app.tabs[0]).defaults.threads == 3);
    let weak = app.download_manager.clone();
    app.tabs.remove(0);
    assert!(weak.upgrade().is_some());
    state(&app.tabs[0])
        .manager
        .as_ref()
        .unwrap()
        .send(tomato_services::downloader::Action::Limit(2))?;
    app.tabs.clear();
    assert!(weak.upgrade().is_none());
    // Exercise immediate close/reopen rather than sleeping before the next open.
    for _ in 0..12 {
        app.open_connection(saved.clone());
        until(&mut app, |app| state(&app.tabs[0]).loaded_settings);
        assert_eq!(state(&app.tabs[0]).defaults.threads, 3);
        app.tabs.clear();
    }
    assert!(app.download_manager.upgrade().is_none());
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(history.with_extension("json.rust-lock"))?;
    let start = Instant::now();
    while lease.try_lock().is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "Downloader actor retained the history lease"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(history)?)?;
    assert_eq!(json["future"]["keep"], 43);
    assert_eq!(json["tasks"][0]["futureRow"]["keep"], 42);
    assert_eq!(std::fs::read(config)?, before);
    Ok(())
}
