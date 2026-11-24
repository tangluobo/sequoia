use super::*;
#[test]
fn adb_independent_window_keeps_session_and_can_return_to_its_tab() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut connection = Connection::new("ADB");
    connection.name = "owned ADB window fixture".into();
    connection.set("host", "127.0.0.1:0");
    // A legacy program path cannot affect the isolated native session.
    connection.set(
        "adbExecutablePath",
        temporary
            .path()
            .join("missing-adb")
            .to_string_lossy()
            .as_ref(),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(path));
    app.open_connection(connection.clone());
    let id = app.tabs[0].id.clone();
    let session = match &mut app.tabs[0].panel {
        Panel::NativeDesktop {
            session: Some(session),
            view,
            ..
        } => {
            view.size = [720, 1280];
            view.buttons = 1 << 1;
            view.position = Some((25, 36));
            std::ptr::from_ref(&*session)
        }
        _ => panic!("ADB session missing"),
    };
    let bridge = super::super::tests::native_bridge::Bridge::new(&ctx);
    app.handle_intent(&id, Intent::ToggleAdbWindow);
    let output = bridge.root_frame(&ctx, vec![], vec![], |ui| app.rdp_windows(ui.ctx()));
    let child = viewport(&id);
    let window = &output.viewport_output[&child];
    assert_eq!(window.builder.fullscreen, Some(false));
    assert_eq!(window.builder.decorations, Some(true));
    assert!(!app.tabs[0].workspace_visible());
    assert_ne!(app.active.as_deref(), Some(id.as_str()));
    assert!(
        matches!(&app.tabs[0].panel, Panel::NativeDesktop { view, .. }
        if view.pending.iter().any(|input| matches!(
            input, tomato_services::remote_desktop::Input::Pointer { button: Some(1), down: false, .. }
        )))
    );
    app.open_connection(connection.clone());
    assert_eq!(app.tabs.len(), 1);
    assert_ne!(app.active.as_deref(), Some(id.as_str()));
    app.handle_intent(&id, Intent::ToggleAdbWindow);
    assert!(app.tabs[0].workspace_visible());
    assert_eq!(app.active.as_deref(), Some(id.as_str()));
    assert!(
        matches!(&app.tabs[0].panel, Panel::NativeDesktop { session: Some(current), view, .. }
        if std::ptr::eq(current, session) && view.size == [720, 1280])
    );

    app.close_tab(&id, true);
    connection.set("fullscreen", true);
    app.open_connection(connection);
    assert!(
        matches!(&app.tabs[0].panel, Panel::NativeDesktop { view, .. }
        if view.window.as_ref().is_some_and(|window| !window.fullscreen))
    );
    Ok(())
}

#[test]
fn minimized_child_logic_keeps_its_viewport_and_parent_input_separate() {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let id = viewport("owned-minimized-logic");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(
            ui.ctx()
                .show_viewport_app(id, egui::ViewportBuilder::default())
        );
    });
    output.textures_delta.clear();
    let mut raw = egui::RawInput {
        viewport_id: id,
        focused: false,
        ..Default::default()
    };
    raw.viewports.insert(
        id,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            minimized: Some(true),
            ..Default::default()
        },
    );
    let output = ctx.run_logic(&raw, |child| {
        assert_eq!(child.viewport_id(), id);
        assert_eq!(child.parent_viewport_id(), egui::ViewportId::ROOT);
        assert!(child.input(|input| input.viewport().minimized == Some(true)));
        assert!(!child.input(|input| input.raw.focused));
        child.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    });
    assert!(output.viewport_commands[&id].contains(&egui::ViewportCommand::Minimized(false)));
    assert_eq!(ctx.viewport_id(), egui::ViewportId::ROOT);
    assert!(ctx.input(|input| input.raw.focused));
}
#[test]
fn legacy_mode_defaults_and_native_window_lifecycle_preserve_the_tab() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut connection = Connection::new("RDP");
    connection.name = "owned offline RDP fixture".into();
    connection.set("host", "127.0.0.1");
    // Missing password means no protocol connection is created by this UI test.
    Store {
        connections: vec![connection.clone()],
        ..Default::default()
    }
    .save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
    app.settings.extra.remove("rdpOpenMode");
    assert!(independent(&connection, &app.settings));
    app.open_rdp(connection.clone());
    let id = app.tabs[0].id.clone();
    assert!(!app.tabs[0].workspace_visible());
    assert!(matches!(
        &app.tabs[0].panel,
        Panel::NativeDesktop { session: None, .. }
    ));
    app.open_rdp(connection.clone());
    assert_eq!(app.tabs.len(), 1);
    let bridge = super::super::tests::native_bridge::Bridge::new(&ctx);
    let mut registered = bridge.root_frame(&ctx, vec![], vec![], |ui| app.rdp_windows(ui.ctx()));
    let child_output = registered.viewport_output.get(&viewport(&id)).unwrap();
    assert!(matches!(child_output.class, egui::ViewportClass::Deferred));
    assert!(child_output.viewport_ui_cb.is_none());
    registered.textures_delta.clear();
    if let Panel::NativeDesktop { view, .. } = &mut app.tabs[0].panel {
        view.modifiers = egui::Modifiers::CTRL;
        view.position = Some((12, 34));
        view.buttons = 1 << 1;
    }
    app.rdp_action(&id, Action::Fullscreen);
    assert!(
        matches!(&app.tabs[0].panel,Panel::NativeDesktop{view,..}if view.window.as_ref().unwrap().bar_until > ctx.input(|input|input.time))
    );
    assert!(
        matches!(&app.tabs[0].panel,Panel::NativeDesktop{view,..}if view.window.as_ref().unwrap().fullscreen)
    );
    if let Panel::NativeDesktop { view, .. } = &mut app.tabs[0].panel {
        assert!(!view.modifiers.ctrl);
        assert_eq!(view.buttons, 0);
        assert!(view.pending.iter().any(|input| matches!(
            input,
            tomato_services::remote_desktop::Input::ScanCode {
                code: 0x1d,
                down: false,
                ..
            }
        )));
        view.window.as_mut().unwrap().pinned = true;
        view.window.as_mut().unwrap().bar_x = 64.;
    }
    app.rdp_action(&id, Action::Restore);
    assert!(
        matches!(&app.tabs[0].panel,Panel::NativeDesktop{view,..}if !view.window.as_ref().unwrap().pinned&&view.window.as_ref().unwrap().bar_x==0.)
    );
    assert!(!app.tabs[0].workspace_visible());
    connection.set("rdpOpenMode", "TAB");
    app.open_rdp(connection.clone());
    assert_eq!(app.tabs.len(), 2);
    let tab = app.tabs[1].id.clone();
    assert!(app.tabs[1].workspace_visible());
    app.rdp_action(&tab, Action::Fullscreen);
    assert!(!app.tabs[1].workspace_visible());
    app.rdp_action(&tab, Action::Restore);
    assert!(app.tabs[1].workspace_visible());
    assert_eq!(app.active.as_deref(), Some(tab.as_str()));
    // The child now paints independently, without invoking the parent's UI.
    let child = viewport(&id);
    let mut input = egui::RawInput {
        viewport_id: child,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800., 600.),
        )),
        ..Default::default()
    };
    input.viewports.insert(
        child,
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            events: vec![egui::ViewportEvent::Close],
            ..Default::default()
        },
    );
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    assert!(
        !output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|root| root.commands.contains(&egui::ViewportCommand::Close))
    );
    output.textures_delta.clear();
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].id, tab);
    app.settings
        .extra
        .insert("rdpOpenMode".into(), serde_json::json!("TAB"));
    connection.options.remove("rdpOpenMode");
    assert!(!independent(&connection, &app.settings));
    connection.set("rdpOpenMode", "WINDOW");
    assert!(independent(&connection, &app.settings));
    connection.set("rdpOpenMode", "tab");
    assert!(!independent(&connection, &app.settings));
    Ok(())
}
#[test]
fn session_dialog_cancel_and_save_preserve_encrypted_legacy_unknown_fields() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut connection = Connection::new("RDP");
    connection.set("future", serde_json::json!({"nested":[1,2,3]}));
    connection.set("rdpOpenMode", "TAB");
    Store {
        connections: vec![connection.clone()],
        ..Default::default()
    }
    .save(&path)?;
    let original = std::fs::read(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
    app.open_rdp(connection.clone());
    let id = app.tabs[0].id.clone();
    app.open_session_config(&id);
    let bridge = super::super::tests::native_bridge::Bridge::new(&ctx);
    bridge.frame(&ctx, vec![], |ctx| app.session_config_dialog(ctx));
    bridge.click(&ctx, bridge.text("取消")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    assert!(app.session_config.is_none());
    assert_eq!(std::fs::read(&path)?, original);
    app.open_session_config(&id);
    bridge.frame(&ctx, vec![], |ctx| app.session_config_dialog(ctx));
    bridge.click(&ctx, bridge.text("连接标签页")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    bridge.click(&ctx, bridge.text("独立窗口")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    bridge.click(&ctx, bridge.text("确定")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    assert!(app.session_config.is_none());
    let saved = Store::load(&path)?;
    assert_eq!(saved.connections[0].text("rdpOpenMode"), "WINDOW");
    assert_eq!(
        saved.connections[0].options.get("future"),
        connection.options.get("future")
    );
    assert!(!String::from_utf8_lossy(&std::fs::read(&path)?).contains("nested"));
    app.open_session_config(&id);
    let mut external = saved;
    external.connections[0].name = "external writer".into();
    external.save(&path)?;
    let external_bytes = std::fs::read(&path)?;
    bridge.frame(&ctx, vec![], |ctx| app.session_config_dialog(ctx));
    bridge.click(&ctx, bridge.text("确定")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    assert!(app.session_config.is_some());
    assert_eq!(std::fs::read(&path)?, external_bytes);
    bridge.click(&ctx, bridge.text("取消")?, |ctx| {
        app.session_config_dialog(ctx)
    });
    assert!(app.session_config.is_none());
    Ok(())
}
