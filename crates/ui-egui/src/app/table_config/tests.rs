use super::*;

#[test]
fn actual_table_font_dialog_saves_legacy_keys_preserves_unknowns_and_cancel_does_not_write()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let profile = directory.path().join("global.json");
    std::fs::write(
        &profile,
        br#"{"tableFontName":"Sans Serif","tableFontSize":10,"future":{"keep":true}}"#,
    )?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let original = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app =
        TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(config.clone()));
    app.table_config = Some(Box::new(Dialog::new(TableFont {
        name: "Sans Serif".into(),
        size: 17,
    })));
    let bridge = crate::app::tests::native_bridge::Bridge::new(&ctx);
    bridge.frame(&ctx, vec![], |ctx| app.table_config_dialog(ctx));
    bridge.frame(&ctx, vec![], |ctx| app.table_config_dialog(ctx));
    bridge.click(&ctx, egui::pos2(293., 25.5), |ctx| {
        app.table_config_dialog(ctx)
    });
    assert_eq!(app.table_config.as_ref().unwrap().font.size, 18);
    bridge.click(&ctx, egui::pos2(150., 31.), |ctx| {
        app.table_config_dialog(ctx)
    });
    let family = bridge.text("Arial")?;
    bridge.click(&ctx, family, |ctx| app.table_config_dialog(ctx));
    assert_eq!(app.table_config.as_ref().unwrap().font.name, "Arial");
    let save = bridge.text("确定")?;
    bridge.click(&ctx, save, |ctx| app.table_config_dialog(ctx));
    assert!(app.table_config.is_none(), "{:?}", app.error);
    assert_eq!(app.settings.table_font().size, 18);
    let persisted: serde_json::Value = serde_json::from_slice(&std::fs::read(&profile)?)?;
    assert_eq!(persisted["tableFontSize"], 18);
    assert_eq!(persisted["tableFontName"], "Arial");
    assert_eq!(persisted["future"]["keep"], true);
    assert_eq!(std::fs::read(&config)?, original);
    let saved = std::fs::read(&profile)?;
    app.table_config = Some(Box::new(Dialog::new(TableFont {
        name: "Sans Serif".into(),
        size: 24,
    })));
    bridge.frame(&ctx, vec![], |ctx| app.table_config_dialog(ctx));
    bridge.frame(&ctx, vec![], |ctx| app.table_config_dialog(ctx));
    let cancel = bridge.text("取消")?;
    bridge.click(&ctx, cancel, |ctx| app.table_config_dialog(ctx));
    assert!(app.table_config.is_none());
    assert_eq!(std::fs::read(profile)?, saved);
    Ok(())
}
