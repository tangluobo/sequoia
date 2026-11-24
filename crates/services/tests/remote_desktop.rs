//! No saved connection is used: the endpoint is an isolated, synthetic TPKT peer.
#[path = "support/rdp_fixture.rs"]
mod rdp_fixture;
use anyhow::{Context as _, Result};
use std::time::Duration;
use tokio::{io::AsyncReadExt, net::TcpListener};
use tomato_core::Connection;
use tomato_services::remote_desktop::{DesktopSession, Status};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the owned synthetic FreeRDP loopback sample; never a user desktop"]
async fn independent_freerdp_standard_security_draws_accepts_input_and_closes() -> Result<()> {
    use tomato_services::remote_desktop::Input;
    anyhow::ensure!(
        std::env::var("TOMATO_TEST_FREERDP_SAMPLE").as_deref() == Ok("owned-loopback-synthetic"),
        "private sample fixture required"
    );
    let port = std::env::var("TOMATO_TEST_FREERDP_SAMPLE_PORT")?.parse::<u16>()?;
    anyhow::ensure!(port > 0, "fixture port missing");
    for use_ssl in [false, true] {
        let mut connection = Connection::new("RDP");
        connection.set("host", "127.0.0.1");
        connection.set("port", port);
        connection.set("username", "fixture-user");
        connection.set("password", "fixture-only");
        connection.set("useSsl", use_ssl);
        connection.set("screenWidth", 640);
        connection.set("screenHeight", 480);
        connection.set("mapClipboard", false);
        connection.set("enableSound", false);
        let original = serde_json::to_value(&connection)?;
        let mut session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                session.poll();
                if let Status::Failed(error) = session.status() {
                    anyhow::bail!("{error}");
                }
                if session.status() == Status::Ready
                    && let Some(frame) = session.take_frame()
                    && frame.pixels.iter().any(|pixel| *pixel & 0x00ff_ffff != 0)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        for index in 0..4105 {
            // Cross the key-renewal boundary with native FastPath input. The
            // independent server authenticates every packet and draws its icon.
            let before = session
                .input_dispatch_stats()
                .context("native input lane missing")?
                .messages;
            session.send(Input::ScanCode {
                code: 0x2a,
                extended: false,
                down: index % 2 == 0,
            })?;
            tokio::time::timeout(Duration::from_secs(2), async {
                while session.input_dispatch_stats().unwrap().messages == before {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await?;
            session.poll();
            if let Status::Failed(error) = session.status() {
                anyhow::bail!("{error}");
            }
        }
        session.send(Input::ScanCode {
            code: 0x2a,
            extended: false,
            down: false,
        })?;
        session.take_frame();
        session.send(Input::Pointer {
            x: 321,
            y: 123,
            button: None,
            down: false,
            wheel: 0,
        })?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                session.poll();
                if let Status::Failed(error) = session.status() {
                    anyhow::bail!("{error}");
                }
                if session.take_frame().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        session.poll();
        anyhow::ensure!(
            session.status() == Status::Ready,
            "sample disconnected after encrypted input"
        );
        let sent = session.input_dispatch_stats().unwrap().messages;
        let closing = std::time::Instant::now();
        drop(session);
        tokio::time::timeout(
            Duration::from_secs(2),
            tomato_services::remote_desktop::wait_rdp_cleanup(),
        )
        .await?;
        anyhow::ensure!(
            closing.elapsed() < Duration::from_secs(2),
            "standard RDP close blocked"
        );
        println!(
            "standard-security-acceptance initial_tls={use_ssl} sent={sent} cleanup_ms={}",
            closing.elapsed().as_secs_f64() * 1000.
        );
        anyhow::ensure!(
            serde_json::to_value(&connection)? == original,
            "saved connection changed"
        );
    }
    Ok(())
}

#[tokio::test]
async fn native_rdp_starts_protocol_and_reports_a_rejected_handshake() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut header = [0; 4];
        socket.read_exact(&mut header).await?;
        anyhow::ensure!(
            header[..2] == [3, 0],
            "Expected native RDP TPKT negotiation"
        );
        Ok::<_, anyhow::Error>(())
    });
    let mut connection = Connection::new("RDP");
    connection.set("host", "127.0.0.1");
    connection.set("port", address.port());
    connection.set("username", "fixture");
    connection.set("password", "fixture-only");
    connection.set("mapClipboard", false);
    let mut session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            session.poll();
            match session.status() {
                Status::Failed(message) => {
                    assert!(message.contains("RDP"));
                    break;
                }
                Status::Closed => anyhow::bail!("The handshake error was lost"),
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    peer.await??;
    Ok(())
}

#[tokio::test]
async fn closing_a_connecting_native_rdp_tab_closes_its_socket() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut connection = Connection::new("RDP");
    connection.set("port", listener.local_addr()?.port());
    connection.set("username", "fixture");
    connection.set("password", "fixture-only");
    connection.set("mapClipboard", false);
    let mut session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        drive(&mut session, listener.accept()),
    )
    .await??;
    let mut header = [0; 4];
    drive(&mut session, socket.read_exact(&mut header)).await?;
    let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let mut rest = vec![0; length - 4];
    drive(&mut session, socket.read_exact(&mut rest)).await?;
    drop(session);
    let mut next = [0];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), socket.read(&mut next)).await??,
        0
    );
    Ok(())
}

async fn drive<T>(session: &mut DesktopSession, work: impl std::future::Future<Output = T>) -> T {
    tokio::pin!(work);
    loop {
        session.poll();
        tokio::select! {
            result = &mut work => return result,
            _ = tokio::time::sleep(Duration::from_millis(1)) => {},
        }
    }
}
