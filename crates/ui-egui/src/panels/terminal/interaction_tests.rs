use super::*;

fn read(tab: &mut Tab) {
    let Panel::Terminal {
        session,
        parser,
        view,
        ..
    } = &mut tab.panel
    else {
        unreachable!()
    };
    while let Some(bytes) = session.try_read() {
        for reply in view.control.process(&bytes, parser).replies {
            session.write(&reply).unwrap();
        }
    }
}
fn wait(tab: &mut Tab, text: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        read(tab);
        if let Panel::Terminal { parser, .. } = &tab.panel
            && parser.screen().contents().contains(text)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture terminal did not produce {text}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn clipboard_actions_leave_the_terminal_ready_for_keyboard_input() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut connection = tomato_core::Connection::new("LOCAL_TERMINAL");
    connection.set("terminalType", "cmd");
    connection.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut tab = crate::tabs::create_tab(connection, Some(runtime.handle()))?;
    let Panel::Terminal { session, .. } = &mut tab.panel else {
        unreachable!()
    };
    session.write("\x1b[1;1R")?;
    exercise_clipboard_actions(&mut tab);
    if let Panel::Terminal { session, .. } = tab.panel {
        runtime.block_on(session.shutdown());
    }
    Ok(())
}

#[test]
fn mouse_wheel_reaches_deep_history_after_a_real_shell_command() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut connection = tomato_core::Connection::new("LOCAL_TERMINAL");
    connection.set("terminalType", "cmd");
    connection.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut tab = crate::tabs::create_tab(connection, Some(runtime.handle()))?;
    let Panel::Terminal { session, .. } = &mut tab.panel else {
        unreachable!()
    };
    session.write("\x1b[1;1R")?;
    wait(&mut tab, ">");

    let ctx = egui::Context::default();
    let mut icons = crate::icons::Icons::default();
    let mut frame = |tab: &mut Tab, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                time: Some(ctx.input(|input| input.time) + 0.2),
                ..Default::default()
            },
            |ui| {
                render(ui, tab, &mut icons);
            },
        );
        output.textures_delta.clear();
    };
    frame(&mut tab, vec![]);
    if let Panel::Terminal { session, .. } = &mut tab.panel {
        session.write("for /L %i in (1,1,160) do @echo scroll-row-%i\r")?;
    }
    wait(&mut tab, "scroll-row-160");
    frame(
        &mut tab,
        vec![egui::Event::PointerMoved(egui::pos2(200., 200.))],
    );

    for (delta, reading_history) in [(8., true), (-16., false)] {
        frame(
            &mut tab,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Page,
                delta: egui::vec2(0., delta),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        // Let the wheel movement settle before checking the end position.
        for _ in 0..12 {
            frame(&mut tab, vec![]);
        }
        let Panel::Terminal { parser, view, .. } = &tab.panel else {
            unreachable!()
        };
        let screen = parser.screen();
        assert_eq!(view.paused_output, reading_history);
        if reading_history {
            assert!(screen.scrollback() > usize::from(screen.size().0));
            assert!(!screen.contents().contains("scroll-row-160"));
        } else {
            assert_eq!(screen.scrollback(), 0);
            assert!(screen.contents().contains("scroll-row-160"));
        }
    }
    if let Panel::Terminal { session, .. } = tab.panel {
        runtime.block_on(session.shutdown());
    }
    Ok(())
}

#[test]
#[ignore = "run from the disposable SSH workspace fixture with its isolated profile"]
fn clipboard_actions_deliver_follow_up_input_over_ssh() -> anyhow::Result<()> {
    let profile = std::env::var_os("TOMATO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Missing isolated fixture profile"))?;
    let mut store = tomato_core::Store::load(&profile.join("connections.json"))?;
    anyhow::ensure!(
        store.connections.len() == 1,
        "Expected one fixture connection"
    );
    let connection = store.connections.remove(0);
    anyhow::ensure!(
        connection.kind() == "SSH"
            && connection.name == "SSH 本地验收"
            && connection.text("username") == "fixture"
            && matches!(connection.text("host"), "" | "127.0.0.1"),
        "Expected an isolated loopback SSH fixture"
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut tab = crate::tabs::create_tab(connection, Some(runtime.handle()))?;
    wait(&mut tab, "fixture$");
    exercise_clipboard_actions(&mut tab);
    wait(&mut tab, "fixture$ fixtureafterpasted");
    wait(&mut tab, "typed");
    if let Panel::Terminal { session, .. } = tab.panel {
        runtime.block_on(session.shutdown());
    }
    Ok(())
}

fn exercise_clipboard_actions(tab: &mut Tab) {
    use egui::{Event, Key, Modifiers, PointerButton};

    let Panel::Terminal { view, .. } = &mut tab.panel else {
        unreachable!()
    };
    view.selection_snapshot = Some("fixture".into());

    let ctx = egui::Context::default();
    let mut icons = crate::icons::Icons::default();
    let mut frame = |mut events: Vec<Event>, modifiers| {
        events.insert(0, Event::ModifiersChanged(modifiers));
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                time: Some(ctx.input(|i| i.time) + 0.2),
                ..Default::default()
            },
            |ui| {
                render(ui, tab, &mut icons);
            },
        );
        out.textures_delta.clear();
        out
    };
    let pointer = |pos, button, pressed| Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let click = |frame: &mut dyn FnMut(Vec<Event>, Modifiers) -> egui::FullOutput, pos, button| {
        frame(vec![Event::PointerMoved(pos)], Modifiers::NONE);
        frame(vec![pointer(pos, button, true)], Modifiers::NONE);
        frame(vec![pointer(pos, button, false)], Modifiers::NONE)
    };
    let menu_origin = egui::pos2(200., 200.);
    let choose_menu_item = |frame: &mut dyn FnMut(Vec<Event>, Modifiers) -> egui::FullOutput,
                            label| {
        click(frame, menu_origin, PointerButton::Secondary);
        let menu = frame(vec![], Modifiers::NONE);
        let position = menu.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.)
            }
            _ => None,
        });
        click(
            frame,
            position.unwrap_or_else(|| panic!("missing menu item {label}")),
            PointerButton::Primary,
        )
    };

    let copied = choose_menu_item(&mut frame, "复制");
    assert!(copied.platform_output.ime.is_some());
    assert!(copied.platform_output.commands.iter().any(|command| {
        matches!(command, egui::OutputCommand::CopyText(text) if text == "fixture")
    }));
    let ctrl = Modifiers::CTRL | Modifiers::COMMAND;
    let copied_by_key = frame(vec![Event::Copy], ctrl);
    assert!(
        copied_by_key
            .platform_output
            .commands
            .iter()
            .any(|command| {
                matches!(command, egui::OutputCommand::CopyText(text) if text == "fixture")
            })
    );
    assert!(
        choose_menu_item(&mut frame, "复制并粘贴")
            .platform_output
            .ime
            .is_some()
    );
    let follow_up = frame(vec![Event::Text("after".into())], Modifiers::NONE);
    assert!(follow_up.platform_output.ime.is_some());
    let requested = choose_menu_item(&mut frame, "粘贴");
    assert!(
        requested.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::RequestPaste)
    );
    assert!(requested.platform_output.ime.is_some());

    // The Windows backend emits semantic clipboard events; Shift may have been released by this frame.
    let pasted = frame(vec![Event::Paste("pasted".into())], ctrl);
    assert!(pasted.platform_output.ime.is_some());
    let entered = frame(
        vec![
            Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Text("typed".into()),
        ],
        Modifiers::NONE,
    );
    assert!(entered.platform_output.ime.is_some());
    if let Panel::Terminal { view, .. } = &tab.panel {
        assert_eq!(view.input_line, "afterpastedtyped");
        assert!(!view.has_selection());
    }
}

#[test]
fn actual_local_clear_menu_preserves_the_pending_shell_command() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut c = tomato_core::Connection::new("LOCAL_TERMINAL");
    c.set("terminalType", "cmd");
    c.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut tab = crate::tabs::create_tab(c, Some(runtime.handle()))?;
    if let Panel::Terminal { session, .. } = &mut tab.panel {
        session.write("\x1b[1;1R")?;
    }
    wait(&mut tab, ">");
    if let Panel::Terminal { session, .. } = &mut tab.panel {
        session.write("echo TOMATO_CLEAR_PENDING")?;
    }
    wait(&mut tab, "TOMATO_CLEAR_PENDING");
    let ctx = egui::Context::default();
    let mut icons = crate::icons::Icons::default();
    let mut frame = |events| {
        read(&mut tab);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                time: Some(ctx.input(|i| i.time) + 0.2),
                ..Default::default()
            },
            |ui| {
                render(ui, &mut tab, &mut icons);
            },
        );
        out.textures_delta.clear();
        out
    };
    frame(vec![]);
    frame(vec![]);
    let pos = egui::pos2(200., 200.);
    frame(vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    let out = frame(vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "清屏" => {
                Some(t.pos + t.galley.size() / 2.)
            }
            _ => None,
        })
        .unwrap();
    frame(vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    if let Panel::Terminal {
        parser,
        session,
        view,
        ..
    } = &mut tab.panel
    {
        assert!(parser.screen().contents().trim().is_empty());
        assert!(!view.has_selection());
        assert!(!view.paused_output);
        session.write("\r")?;
    }
    wait(&mut tab, "TOMATO_CLEAR_PENDING");
    if let Panel::Terminal { session, .. } = tab.panel {
        runtime.block_on(session.shutdown());
    }
    Ok(())
}
