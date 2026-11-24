#[path = "support/adb_peer.rs"]
mod adb_peer;
use adb_peer::Fixture;
use anyhow::{Result, ensure};
use std::time::Duration;
use tomato_services::remote_desktop::{DesktopSession, Input, Status};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the owned loopback adb_fixture example"]
async fn native_adb_stream_control_and_disconnect() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let exe = Fixture::start(directory.path())?;
    let mut connection = exe.connection();
    connection.set("adbDisplayMode", "PNG");
    connection.set("adbExecutablePath", "missing/legacy-adb");
    let mut session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            session.poll();
            if let Some(frame) = session.take_frame() {
                assert_eq!((frame.width, frame.height), (96, 64));
                assert_eq!(frame.pixels[0], 127);
                break;
            }
            if let Status::Failed(e) = session.status() {
                anyhow::bail!(e);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    assert_eq!(session.status(), Status::Ready);
    for event in [
        Input::AndroidKey(3),
        Input::Pointer {
            x: 10,
            y: 20,
            button: Some(1),
            down: true,
            wheel: 0,
        },
        Input::Pointer {
            x: 10,
            y: 20,
            button: Some(1),
            down: false,
            wheel: 0,
        },
        Input::Unicode {
            value: 39,
            down: true,
        },
        Input::Unicode {
            value: 32,
            down: true,
        },
        Input::ScanCode {
            code: 15,
            extended: false,
            down: true,
        },
    ] {
        session.send(event)?;
    }
    let log = exe.with_extension("commands.jsonl");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            session.poll();
            let text = std::fs::read_to_string(&log)?;
            if text.contains("keyevent '61'") || text.contains("'keyevent' '61'") {
                break;
            }
            if let Status::Failed(e) = session.status() {
                anyhow::bail!(e);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let commands = std::fs::read_to_string(&log)?;
    ensure!(commands.contains("'tap' '10' '20'"), "Tap not delivered");
    ensure!(commands.contains("'keyevent' '3'"), "Home not delivered");
    ensure!(commands.contains("%s"), "Space not escaped");
    drop(session);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let marker = exe.with_extension("heartbeat");
    let stopped = std::fs::read(&marker)?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(stopped, std::fs::read(marker)?);
    Ok(())
}
