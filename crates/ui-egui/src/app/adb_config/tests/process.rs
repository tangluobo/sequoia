use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires the owned loopback adb_fixture example"]
fn legacy_program_paths_cannot_override_native_adb_on_open_or_reconnect() -> anyhow::Result<()> {
    let (root, mut app, _) = fixture()?;
    let peer = adb_peer::Fixture::start(root.path())?;
    let connection_path = root.path().join("connections.json");
    let global_path = root.path().join("global.json");
    let before = (
        std::fs::read(&connection_path)?,
        std::fs::read(&global_path)?,
    );
    let mut connection = app.store.connections[0].clone();
    connection.set("host", "127.0.0.1");
    connection.set("port", peer.connection().number("port"));
    connection.set("adbDisplayMode", "PNG");
    connection.set("fullscreen", true);
    app.open_connection(connection);
    let id = app.tabs.last().unwrap().id.clone();
    assert!(!app.tabs[0].workspace_visible());
    assert!(
        matches!(&app.tabs[0].panel, Panel::NativeDesktop { view, .. }
        if view.window.as_ref().is_some_and(|window| !window.fullscreen))
    );
    ready(&mut app)?;
    app.connect_desktop(&id);
    ready(&mut app)?;
    let Panel::NativeDesktop {
        session: Some(session),
        connection,
        ..
    } = &app.tabs[0].panel
    else {
        anyhow::bail!("Missing native ADB session");
    };
    assert!(!connection.text("adbExecutablePath").is_empty());
    session.send(tomato_services::remote_desktop::Input::AndroidKey(3))?;
    let start = Instant::now();
    let log = peer.with_extension("commands.jsonl");
    loop {
        let log = std::fs::read_to_string(&log)?;
        if log.contains("'keyevent' '3'")
            && log.lines().filter(|line| *line == r#"["connect"]"#).count() == 2
        {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(10),
            "Native reconnect or control did not reach the peer"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        (std::fs::read(connection_path)?, std::fs::read(global_path)?),
        before
    );
    Ok(())
}

fn ready(app: &mut TomatoApp) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        let Panel::NativeDesktop {
            session: Some(session),
            ..
        } = &mut app.tabs[0].panel
        else {
            anyhow::bail!("Missing ADB session");
        };
        session.poll();
        if let Some(frame) = session.take_frame() {
            assert_eq!((frame.width, frame.height), (96, 64));
            return Ok(());
        }
        if let tomato_services::remote_desktop::Status::Failed(error) = session.status() {
            anyhow::bail!(error);
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(10),
            "ADB initial/reconnect timeout"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
