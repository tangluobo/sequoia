use super::*;

fn fixture() -> anyhow::Result<(tempfile::TempDir, TomatoApp, Vec<String>)> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let connections = (0..2)
        .map(|index| {
            let mut connection = Connection::new("RDP");
            connection.name = format!("owned close fixture {index}");
            connection.set("host", "127.0.0.1");
            connection.set("rdpOpenMode", "WINDOW");
            connection
        })
        .collect::<Vec<_>>();
    Store {
        connections: connections.clone(),
        ..Default::default()
    }
    .save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let context = egui::Context::default();
    context.set_embed_viewports(false);
    let mut app =
        TomatoApp::with_repository(&context, runtime, ConnectionFileRepository::new(path));
    for connection in connections {
        app.open_rdp(connection);
    }
    let ids = app
        .tabs
        .iter()
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| app.rdp_windows(ui.ctx()));
    output.textures_delta.clear();
    Ok((directory, app, ids))
}

fn close_input(id: &str, current: egui::ViewportId, minimized: bool) -> egui::RawInput {
    let mut input = egui::RawInput {
        viewport_id: current,
        ..Default::default()
    };
    input.viewports.insert(
        viewport(id),
        egui::ViewportInfo {
            parent: Some(egui::ViewportId::ROOT),
            minimized: Some(minimized),
            occluded: Some(minimized),
            events: vec![egui::ViewportEvent::Close],
            ..Default::default()
        },
    );
    input
}

#[test]
fn minimized_or_occluded_close_is_processed_without_a_ui_pass() -> anyhow::Result<()> {
    for current_is_root in [false, true] {
        let (directory, mut app, ids) = fixture()?;
        let original = std::fs::read(directory.path().join("connections.json"))?;
        let context = app.context.clone();
        let current = if current_is_root {
            egui::ViewportId::ROOT
        } else {
            viewport(&ids[0])
        };
        let output = context.run_logic(&close_input(&ids[0], current, true), |ctx| app.logic(ctx));
        assert_eq!(
            app.tabs.len(),
            1,
            "close must not wait for the hidden window to paint"
        );
        assert_eq!(app.tabs[0].id, ids[1]);
        assert!(
            output
                .viewport_commands
                .get(&viewport(&ids[0]))
                .is_some_and(|cmds| cmds.contains(&egui::ViewportCommand::Close))
        );
        assert!(
            !output
                .viewport_commands
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|cmds| cmds.contains(&egui::ViewportCommand::Close))
        );
        assert_eq!(
            std::fs::read(directory.path().join("connections.json"))?,
            original
        );
    }
    Ok(())
}

#[test]
fn closing_window_dismisses_only_its_own_session_dialog() -> anyhow::Result<()> {
    for close_owner in [false, true] {
        let (_directory, mut app, ids) = fixture()?;
        app.open_session_config(&ids[0]);
        let closing = &ids[usize::from(!close_owner)];
        let context = app.context.clone();
        let mut output = context.run_ui(close_input(closing, viewport(closing), false), |ui| {
            app.ui(ui)
        });
        output.textures_delta.clear();
        assert_eq!(
            app.tabs.len(),
            1,
            "a session settings dialog must not cancel a window close"
        );
        assert_eq!(
            app.session_config.is_none(),
            close_owner,
            "other session settings must stay open"
        );
        assert!(
            !output
                .viewport_output
                .get(&viewport(closing))
                .is_some_and(|vp| vp.commands.contains(&egui::ViewportCommand::CancelClose))
        );
    }
    Ok(())
}

#[test]
fn closing_a_minimized_connecting_window_cancels_the_real_pending_socket() -> anyhow::Result<()> {
    use std::{
        io::Read as _,
        time::{Duration, Instant},
    };
    let (_directory, mut app, ids) = fixture()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let Panel::NativeDesktop {
        connection, view, ..
    } = &mut app.tabs[0].panel
    else {
        unreachable!()
    };
    connection.set("port", listener.local_addr()?.port());
    connection.set("mapClipboard", false);
    view.password = "owned-loopback-only".into();
    app.connect_desktop(&ids[0]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut peer = loop {
        match listener.accept() {
            Ok((peer, _)) => break peer,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(Instant::now() < deadline, "owned socket never connected");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.into()),
        }
    };
    peer.set_nonblocking(true)?;
    let context = app.context.clone();
    let _ = context.run_logic(&close_input(&ids[0], viewport(&ids[0]), true), |ctx| {
        app.logic(ctx)
    });
    assert_eq!(app.tabs.len(), 1);
    let mut bytes = [0; 4096];
    loop {
        match peer.read(&mut bytes) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => return Err(error.into()),
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "close did not cancel the pending handshake"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[test]
fn late_close_for_a_restored_tab_does_not_close_its_session_or_other_windows() -> anyhow::Result<()>
{
    let (_directory, mut app, ids) = fixture()?;
    if let Panel::NativeDesktop { view, .. } = &mut app.tabs[0].panel {
        view.window = None;
    }
    let context = app.context.clone();
    let _ = context.run_logic(
        &close_input(&ids[0], egui::ViewportId::ROOT, false),
        |ctx| app.logic(ctx),
    );
    assert_eq!(app.tabs.len(), 2);
    Ok(())
}

#[test]
fn failed_connection_closes_in_logic_without_reopening_the_password_prompt() -> anyhow::Result<()> {
    use std::time::{Duration, Instant};
    let (directory, mut app, ids) = fixture()?;
    let original = std::fs::read(directory.path().join("connections.json"))?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let Panel::NativeDesktop {
        connection, view, ..
    } = &mut app.tabs[0].panel
    else {
        unreachable!()
    };
    connection.set("port", port);
    connection.set("mapClipboard", false);
    view.password = "owned-loopback-only".into();
    app.connect_desktop(&ids[0]);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let Panel::NativeDesktop {
            session: Some(session),
            ..
        } = &mut app.tabs[0].panel
        else {
            anyhow::bail!("missing owned session")
        };
        session.poll();
        if matches!(
            session.status(),
            tomato_services::remote_desktop::Status::Failed(_)
        ) {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "refused endpoint did not fail");
        std::thread::sleep(Duration::from_millis(5));
    }
    let context = app.context.clone();
    let _ = context.run_logic(&close_input(&ids[0], viewport(&ids[0]), true), |ctx| {
        app.logic(ctx)
    });
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(
        std::fs::read(directory.path().join("connections.json"))?,
        original
    );
    Ok(())
}
