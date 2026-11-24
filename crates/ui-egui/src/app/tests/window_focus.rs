use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, ViewportCommand, ViewportId};

fn frame(app: &mut TomatoApp, maximized: bool, events: Vec<Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            if maximized {
                egui::vec2(1500., 940.)
            } else {
                egui::vec2(1200., 900.)
            },
        )),
        time: Some(ctx.input(|input| input.time) + 0.08),
        events,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .maximized = Some(maximized);
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(&ctx);
        app.ui(ui);
    });
    output.textures_delta.clear();
    output
}

fn click(app: &mut TomatoApp, maximized: bool, pos: Pos2) -> egui::FullOutput {
    let events = |pressed| {
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ]
    };
    frame(app, maximized, events(true));
    frame(app, maximized, events(false))
}

fn input(app: &mut TomatoApp, maximized: bool, typed: &str) -> egui::FullOutput {
    frame(
        app,
        maximized,
        vec![
            Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text(typed.into()),
        ],
    )
}

fn active_terminal(app: &TomatoApp) -> &Tab {
    app.tabs
        .iter()
        .find(|tab| Some(&tab.id) == app.active.as_ref())
        .unwrap()
}

fn wait_for_terminal(app: &mut TomatoApp, needle: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        frame(app, false, vec![]);
        if let Panel::Terminal { parser, .. } = &active_terminal(app).panel
            && parser.screen().contents().contains(needle)
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "terminal did not return {needle}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn app_at(config: &std::path::Path) -> anyhow::Result<TomatoApp> {
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    Ok(TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(config.to_owned()),
    ))
}

fn maximize_restore(app: &mut TomatoApp) -> egui::Id {
    for _ in 0..3 {
        assert!(frame(app, false, vec![]).platform_output.ime.is_some());
    }
    let keyboard = app.context.memory(|memory| memory.focused()).unwrap();
    let header = app.layout.header;
    let output = click(
        app,
        false,
        egui::pos2(header.right() - 47., header.top() + 13.),
    );
    assert!(
        output.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Maximized(true))
    );
    assert!(
        output.platform_output.ime.is_some(),
        "maximize click dropped terminal input focus"
    );
    assert!(
        input(app, true, "after-maximize")
            .platform_output
            .ime
            .is_some()
    );
    assert_eq!(
        app.context.memory(|memory| memory.focused()),
        Some(keyboard)
    );

    // The synthetic maximize click must be outside egui's triple-click interval.
    for _ in 0..8 {
        frame(app, true, vec![]);
    }
    let caption = app.layout.header.center();
    let first = click(app, true, caption);
    assert!(first.platform_output.ime.is_some());
    assert!(
        !first.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Maximized(false))
    );
    let output = click(app, true, caption);
    assert!(
        output.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Maximized(false))
    );
    assert!(
        output.platform_output.ime.is_some(),
        "restore click dropped terminal input focus"
    );
    assert!(
        input(app, false, "after-restore")
            .platform_output
            .ime
            .is_some()
    );
    assert_eq!(
        app.context.memory(|memory| memory.focused()),
        Some(keyboard)
    );
    let Panel::Terminal { view, .. } = &active_terminal(app).panel else {
        unreachable!()
    };
    assert_eq!(view.input_line, "after-maximizeafter-restore");
    keyboard
}

#[test]
fn terminal_window_maximize_and_restore_keep_the_keyboard() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = app_at(&directory.path().join("connections.json"))?;
    let mut connection = Connection::new("LOCAL_TERMINAL");
    connection.set("terminalType", "cmd");
    connection.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut tab = tabs::create_tab(
        connection,
        app.runtime.as_ref().map(|runtime| runtime.handle()),
    )?;
    if let Panel::Terminal { session, .. } = &mut tab.panel {
        session.write("\x1b[1;1R")?;
    }
    app.active = Some(tab.id.clone());
    app.tabs.push(tab);
    wait_for_terminal(&mut app, ">");
    let keyboard = maximize_restore(&mut app);
    let output = frame(&mut app, false, vec![]);
    let search = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "搜索" => {
                Some(text.pos + text.galley.size() / 2.)
            }
            _ => None,
        })
        .expect("connection search box");
    click(&mut app, false, search);
    assert!(!app.context.memory(|memory| memory.has_focus(keyboard)));
    if let Panel::Terminal { session, .. } = app.tabs.pop().unwrap().panel {
        app.runtime.as_ref().unwrap().block_on(session.shutdown());
    }
    Ok(())
}

#[test]
#[ignore = "run from the disposable SSH workspace fixture with its isolated profile"]
fn container_terminal_window_maximize_and_restore_send_input_over_ssh() -> anyhow::Result<()> {
    let profile = std::env::var_os("TOMATO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Missing isolated fixture profile"))?;
    let mut app = app_at(&profile.join("connections.json"))?;
    let [connection] = app.store.connections.as_slice() else {
        anyhow::bail!("Expected one fixture connection")
    };
    anyhow::ensure!(
        connection.kind() == "SSH"
            && connection.name == "SSH 本地验收"
            && connection.text("username") == "fixture"
            && matches!(connection.text("host"), "" | "127.0.0.1"),
        "Expected an isolated loopback SSH fixture"
    );
    let command =
        tomato_services::terminal::ssh::tools::management::container_terminal("fixture", false)?;
    app.handle_intent(
        "fixture",
        Intent::SshCommand {
            connection: connection.clone(),
            command,
            title: "进入终端 - fixture".into(),
        },
    );
    wait_for_terminal(&mut app, "docker exec");
    maximize_restore(&mut app);
    wait_for_terminal(&mut app, "after-maximize");
    wait_for_terminal(&mut app, "after-restore");
    if let Panel::Terminal { session, .. } = app.tabs.pop().unwrap().panel {
        app.runtime.as_ref().unwrap().block_on(session.shutdown());
    }
    Ok(())
}
