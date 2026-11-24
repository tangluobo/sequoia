//! A disposable TLS + CredSSP RDP desktop, never a saved user connection.
use anyhow::{Context, Result};
use ironrdp_pdu::ironrdp_core;
use ironrdp_server::{
    BitmapUpdate, Credentials, DesktopSize, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat,
    RdpServer, RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler, TlsIdentityCtx,
};
use std::{
    num::{NonZeroU16, NonZeroUsize},
    time::Duration,
};
use tokio::sync::mpsc;
use tomato_core::Connection;
use tomato_services::remote_desktop::{DesktopSession, Input, Status};

#[path = "rdpgfx_fixture.rs"]
mod rdpgfx_fixture;
#[path = "ssh_jump.rs"]
mod ssh_jump;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_rdp_during_stalled_ssh_handshake_releases_the_owned_socket() -> Result<()> {
    use tokio::io::AsyncReadExt as _;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut bytes = [0; 1024];
        let count = socket.read(&mut bytes).await?;
        anyhow::ensure!(
            count > 0 && bytes[..count].starts_with(b"SSH-"),
            "SSH identification missing"
        );
        let _ = accepted.send(());
        let count = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut bytes)).await??;
        anyhow::ensure!(count == 0, "cancelled SSH handshake did not close");
        Ok::<_, anyhow::Error>(())
    });
    let temporary = tempfile::tempdir()?;
    let mut jump = Connection::new("SSH");
    jump.set("host", "127.0.0.1");
    jump.set("port", port);
    jump.set("username", "fixture-user");
    jump.set("password", "fixture-only");
    let mut target = Connection::new("RDP");
    target.set("host", "rdp.internal.test");
    target.set("port", 3389);
    target.set("username", "fixture-user");
    target.set("password", "fixture-only");
    target.set("mapClipboard", false);
    let before = serde_json::to_value(&target)?;
    let session = DesktopSession::start_via(
        &target,
        &tokio::runtime::Handle::current(),
        &jump,
        temporary.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(5), ready).await??;
    let closing = std::time::Instant::now();
    drop(session);
    tokio::time::timeout(Duration::from_secs(3), peer).await???;
    anyhow::ensure!(
        closing.elapsed() < Duration::from_secs(2),
        "RDP close waited for SSH handshake timeout"
    );
    anyhow::ensure!(
        serde_json::to_value(&target)? == before,
        "saved target changed"
    );
    Ok(())
}

struct Display;
struct Updates(u8);
#[async_trait::async_trait]
impl RdpServerDisplay for Display {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: 320,
            height: 256,
        }
    }
    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(Updates(0)))
    }
}
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        if self.0 == 0 {
            self.0 = 1;
            Ok(Some(DisplayUpdate::Bitmap(BitmapUpdate {
                x: 0,
                y: 0,
                width: NonZeroU16::new(320).unwrap(),
                height: NonZeroU16::new(256).unwrap(),
                // Raw SurfaceBits has BGRX wire order; use the same source format
                // for both the raw and RemoteFX server encoders.
                format: PixelFormat::BgrX32,
                data: [56, 34, 12, 0].repeat(320 * 256).into(),
                stride: NonZeroUsize::new(320 * 4).unwrap(),
            })))
        } else if self.0 == 1 {
            self.0 = 2;
            Ok(Some(DisplayUpdate::RGBAPointer(
                ironrdp_server::RGBAPointer {
                    cache_index: 0,
                    width: 16,
                    height: 16,
                    hot_x: 1,
                    hot_y: 2,
                    data: [230, 80, 40, 255].repeat(16 * 16),
                },
            )))
        } else {
            std::future::pending().await
        }
    }
}
struct Handler(
    mpsc::UnboundedSender<String>,
    mpsc::Sender<(String, std::time::Instant)>,
);
impl RdpServerInputHandler for Handler {
    fn keyboard(&mut self, event: KeyboardEvent) {
        let _ = self.0.send(format!("{event:?}"));
    }
    fn mouse(&mut self, event: MouseEvent) {
        let event = format!("{event:?}");
        let _ = self.1.try_send((event.clone(), std::time::Instant::now()));
        let _ = self.0.send(event);
    }
}
struct ServerTask(tokio::task::JoinHandle<Result<()>>);
impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct CapturedApp(std::process::Child);
impl Drop for CapturedApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "requires a desktop renderer, TOMATO_RDP_CAPTURE_EXE and TOMATO_RDP_CAPTURE_PATH"]
async fn capture_native_rdp_window() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let executable =
                std::env::var_os("TOMATO_RDP_CAPTURE_EXE").context("Missing capture executable")?;
            let path = std::path::PathBuf::from(
                std::env::var_os("TOMATO_RDP_CAPTURE_PATH").context("Missing capture path")?,
            );
            anyhow::ensure!(
                !path.exists(),
                "Capture output already exists; use a fresh path"
            );
            let remotefx = std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref() == Ok("remotefx");
            let motion = std::env::var("TOMATO_RDP_CAPTURE_MOTION").as_deref() == Ok("1");
            let (fixture, _graphics_producer) = if matches!(
                std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref(),
                Ok("clearcodec" | "adaptive" | "avc420" | "progressive")
            ) {
                let (fixture, producer) = rdpgfx_fixture::capture_fixture(motion).await?;
                (fixture, Some(producer))
            } else if motion {
                (
                    Fixture::with_display(remotefx, AnimatedDisplay).await?,
                    None,
                )
            } else {
                (Fixture::start(remotefx).await?, None)
            };
            let directory = fixture._keys.path().join("app-data");
            std::fs::create_dir(&directory)?;
            let mut connection = fixture.connection.clone();
            connection.set(
                "rdpAvc420",
                std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref() == Ok("avc420"),
            );
            if std::env::var("TOMATO_RDP_CAPTURE_CLIPBOARD").as_deref() == Ok("1") {
                connection.set("mapClipboard", true);
            }
            connection.name = "RDP 本地验收".into();
            connection.set(
                "fullscreen",
                std::env::var("TOMATO_RDP_CAPTURE_FULLSCREEN").as_deref() == Ok("1"),
            );
            connection.set(
                "rdpOpenMode",
                std::env::var("TOMATO_RDP_CAPTURE_MODE").unwrap_or_else(|_| "TAB".into()),
            );
            let workspace = tomato_core::Store {
                version: 1,
                connections: vec![connection],
                ..Default::default()
            };
            // This password belongs only to our disposable loopback peer.
            std::fs::write(
                directory.join("connections.json"),
                serde_json::to_vec(&workspace)?,
            )?;
            let mut application = CapturedApp(
                std::process::Command::new(executable)
                    .env("TOMATO_DATA_DIR", &directory)
                    .env("TOMATO_UI_CAPTURE", &path)
                    .env("TOMATO_UI_PREVIEW", "rdp")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::from(std::fs::File::create(
                        path.with_extension("stderr.log"),
                    )?))
                    .spawn()?,
            );
            std::fs::write(path.with_extension("pid"), application.0.id().to_string())?;
            let launched = std::time::Instant::now();
            let mut first_capture_seen = None;
            let mut close_requested_seen = None;
            let waited = tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    if first_capture_seen.is_none() && path.exists() {
                        first_capture_seen = Some(std::time::Instant::now());
                    }
                    if close_requested_seen.is_none() && path.with_extension("close").exists() {
                        close_requested_seen = Some(std::time::Instant::now());
                    }
                    if let Some(status) = application.0.try_wait()? {
                        anyhow::ensure!(status.success(), "Capture process failed: {status}");
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
            let normal_exit = matches!(&waited, Ok(Ok(())));
            let after_capture = first_capture_seen.map(|seen| seen.elapsed());
            let after_close_requested = close_requested_seen.map(|seen| seen.elapsed());
            std::fs::write(
                path.with_extension("exit.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "normalExit": normal_exit,
                    "processLifetimeMs": launched.elapsed().as_secs_f64() * 1000.,
                    "pngObserved": first_capture_seen.is_some(),
                    "exitAfterPngObservedMs": after_capture.map(|elapsed| elapsed.as_secs_f64() * 1000.),
                    "closeMarkerObserved": close_requested_seen.is_some(),
                    "exitAfterCloseObservedMs": after_close_requested.map(|elapsed| elapsed.as_secs_f64() * 1000.),
                    "scope": "owned GUI process exit, 10 ms polling; close marker is emitted after PNG encoding/writing, immediately before requesting ROOT Close; PNG appearance includes remaining encoding time; legacy executables without markers retain the total 30 s deadline; timeout cleanup is never a successful exit"
                }))?,
            )?;
            waited.context("Native RDP capture timed out")??;
            anyhow::ensure!(
                after_close_requested.is_none_or(|elapsed| elapsed < Duration::from_secs(5)),
                "Native RDP capture did not close normally within five seconds of its close request"
            );
            let full_motion = std::env::var("TOMATO_RDP_CAPTURE_MOTION").as_deref() == Ok("1")
                && matches!(
                    std::env::var("TOMATO_RDP_CAPTURE_PATTERN").as_deref(),
                    Ok("full" | "mosaic")
                );
            let adaptive = matches!(
                std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref(),
                Ok("adaptive" | "avc420" | "progressive")
            );
            let progressive =
                std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref() == Ok("progressive");
            let image = image::open(path)?.into_rgb8();
            let remote_pixels = image
                .pixels()
                .filter(|pixel| {
                    if adaptive {
                        let [r, g, b] = pixel.0;
                        r.abs_diff(g) <= 2
                            && r.abs_diff(b) <= 2
                            && if full_motion {
                                (40..=210).contains(&r)
                            } else if progressive {
                                r.abs_diff(80) <= 2 || r.abs_diff(205) <= 2
                            } else {
                                r.abs_diff(48) <= 2
                            }
                    } else if full_motion {
                        pixel.0[1].abs_diff(34) <= 4
                            && pixel.0[2].abs_diff(56) <= 4
                            && (20..220).contains(&pixel.0[0])
                    } else {
                        pixel
                            .0
                            .into_iter()
                            .zip([12_u8, 34, 56])
                            .all(|(value, expected)| {
                                value.abs_diff(expected) <= if remotefx { 4 } else { 0 }
                            })
                    }
                })
                .count();
            anyhow::ensure!(
                remote_pixels >= 30_000,
                "Capture did not show the native RDP framebuffer ({remote_pixels} matching pixels)"
            );
            Ok(())
        })
        .await
}

struct Fixture {
    connection: Connection,
    server: ServerTask,
    received: mpsc::UnboundedReceiver<String>,
    mouse_received_at: mpsc::Receiver<(String, std::time::Instant)>,
    _keys: tempfile::TempDir,
}
impl Fixture {
    async fn start(remotefx: bool) -> Result<Self> {
        Self::with_display(remotefx, Display).await
    }
    async fn with_display(
        remotefx: bool,
        display: impl RdpServerDisplay + 'static,
    ) -> Result<Self> {
        Self::with_graphics(remotefx, display, None).await
    }
    async fn with_graphics(
        remotefx: bool,
        display: impl RdpServerDisplay + 'static,
        gfx: Option<Box<dyn ironrdp_server::GfxServerFactory>>,
    ) -> Result<Self> {
        Self::with_security(remotefx, display, gfx, false).await
    }
    async fn with_security(
        remotefx: bool,
        mut display: impl RdpServerDisplay + 'static,
        gfx: Option<Box<dyn ironrdp_server::GfxServerFactory>>,
        tls_only: bool,
    ) -> Result<Self> {
        let size = display.size().await;
        tomato_services::init_crypto();
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let directory = tempfile::tempdir()?;
        let cert_path = directory.path().join("fixture.pem");
        let key_path = directory.path().join("fixture-key.pem");
        std::fs::write(&cert_path, certificate.cert.pem())?;
        std::fs::write(&key_path, certificate.signing_key.serialize_pem())?;
        let identity = TlsIdentityCtx::init_from_paths(&cert_path, &key_path)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (send, received) = mpsc::unbounded_channel();
        let (timed_send, mouse_received_at) = mpsc::channel(256);
        let builder = RdpServer::builder().with_addr(address);
        let builder = if tls_only {
            builder.with_tls(identity.make_acceptor()?)
        } else {
            builder.with_hybrid(identity.make_acceptor()?, identity.pub_key)
        };
        let mut server = builder
            .with_input_handler(Handler(send, timed_send))
            .with_display_handler(display)
            .with_gfx_factory(gfx)
            .with_bitmap_codecs(
                ironrdp_pdu::rdp::capability_sets::server_codecs_capabilities(&[if remotefx {
                    "remotefx:on"
                } else {
                    "remotefx:off"
                }])
                .map_err(anyhow::Error::msg)?,
            )
            .build();
        server.set_credentials(Some(Credentials {
            username: "fixture".into(),
            password: "fixture-only".into(),
            domain: Some("EXAMPLE".into()),
        }));
        let server = ServerTask(tokio::task::spawn_local(async move {
            if tls_only {
                use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                let (mut socket, _) = listener.accept().await?;
                let mut header = [0u8; 4];
                socket.read_exact(&mut header).await?;
                let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
                anyhow::ensure!((11..=4096).contains(&length), "invalid negotiation length");
                let mut bytes = header.to_vec();
                bytes.resize(length, 0);
                socket.read_exact(&mut bytes[4..]).await?;
                let request = ironrdp_core::decode::<
                    ironrdp_pdu::x224::X224<ironrdp_pdu::nego::ConnectionRequest>,
                >(&bytes)?
                .0;
                anyhow::ensure!(
                    request.protocol.is_standard_rdp_security(),
                    "first attempt was not legacy"
                );
                let refusal = ironrdp_core::encode_vec(&ironrdp_pdu::x224::X224(
                    ironrdp_pdu::nego::ConnectionConfirm::Failure {
                        code: ironrdp_pdu::nego::FailureCode::SSL_REQUIRED_BY_SERVER,
                    },
                ))?;
                socket.write_all(&refusal).await?;
                socket.shutdown().await?;
            }
            let (socket, _) = listener.accept().await?;
            // Match an interactive transport: otherwise Nagle/delayed ACKs on
            // our tiny synthetic rectangles dominate client refresh measurements.
            socket.set_nodelay(true)?;
            server.run_connection(socket).await
        }));
        let mut connection = Connection::new("RDP");
        connection.set("port", address.port());
        connection.set("username", "fixture");
        connection.set("password", "fixture-only");
        connection.set("domain", "EXAMPLE");
        connection.set("screenWidth", size.width);
        connection.set("screenHeight", size.height);
        connection.set("mapClipboard", false);
        Ok(Self {
            connection,
            server,
            received,
            mouse_received_at,
            _keys: directory,
        })
    }
}

#[tokio::test]
async fn legacy_configuration_retries_an_explicit_tls_requirement_and_preserves_saved_ssl_flag()
-> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::with_security(false, Display, None, true).await?;
            fixture.connection.set("useSsl", false);
            let original = serde_json::to_value(&fixture.connection)?;
            let mut session =
                DesktopSession::start(&fixture.connection, &tokio::runtime::Handle::current())?;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    session.poll();
                    if let Status::Failed(error) = session.status() {
                        anyhow::bail!("{error}");
                    }
                    if session.status() == Status::Ready && session.take_frame().is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                session.send(Input::Pointer {
                    x: 31,
                    y: 47,
                    button: None,
                    down: false,
                    wheel: 0,
                })?;
                let input = tokio::time::timeout(Duration::from_secs(3), fixture.received.recv())
                    .await?
                    .context("TLS retry lost input")?;
                anyhow::ensure!(
                    input.contains("31") && input.contains("47"),
                    "unexpected input: {input}"
                );
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            drop(session);
            tokio::time::timeout(
                Duration::from_secs(2),
                tomato_services::remote_desktop::wait_rdp_cleanup(),
            )
            .await?;
            let _ = tokio::time::timeout(Duration::from_secs(3), &mut fixture.server.0).await?;
            assert_eq!(serde_json::to_value(&fixture.connection)?, original);
            Ok(())
        })
        .await
}

#[tokio::test]
async fn native_rdp_credssp_frame_keyboard_unicode_pointer_and_disconnect() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(exercise_desktop(false))
        .await
}
#[tokio::test]
async fn native_rdp_ssh_jump_keeps_target_identity_and_releases_on_close() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let jump = ssh_jump::Jump::start().await?;
            let mut fixture = Fixture::start(false).await?;
            fixture.connection.set("host", "rdp.internal.test");
            fixture.connection.set("useSshTunnel", true);
            let before = serde_json::to_value(&fixture.connection)?;
            let mut session = DesktopSession::start_via(
                &fixture.connection,
                &tokio::runtime::Handle::current(),
                &jump.connection,
                jump.root.path().join("known_hosts"),
            )?;
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    session.poll();
                    if let Status::Failed(error) = session.status() {
                        anyhow::bail!(error);
                    }
                    if session.status() == Status::Ready && session.take_frame().is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                session.send(Input::Pointer {
                    x: 31,
                    y: 47,
                    button: None,
                    down: false,
                    wheel: 0,
                })?;
                let input = tokio::time::timeout(Duration::from_secs(3), fixture.received.recv())
                    .await?
                    .context("SSH-routed pointer did not reach the desktop")?;
                assert!(input.contains("31") && input.contains("47"), "{input}");
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            assert_eq!(
                jump.targets.lock().unwrap().as_slice(),
                &[(
                    "rdp.internal.test".into(),
                    fixture.connection.number("port") as u32
                )]
            );
            assert_eq!(serde_json::to_value(&fixture.connection)?, before);
            drop(session);
            let _ = tokio::time::timeout(Duration::from_secs(3), &mut fixture.server.0).await?;
            Ok(())
        })
        .await
}
#[tokio::test]
async fn native_rdp_remotefx_frame_and_input() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(exercise_desktop(true))
        .await
}
#[tokio::test]
async fn native_rdp_cursor_shape_is_separate_and_mouse_input_does_not_wait_for_graphics()
-> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let Fixture {
                connection,
                server,
                mut received,
                _keys,
                ..
            } = Fixture::start(false).await?;
            let mut session =
                DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    session.poll();
                    if let Status::Failed(error) = session.status() {
                        anyhow::bail!("{error}");
                    }
                    if session.status() == Status::Ready && session.pointer().1.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            let (_, pointer) = session.pointer();
            let pointer = pointer.unwrap();
            assert_eq!(pointer.size, [16, 16]);
            assert_eq!(pointer.hotspot, [1, 2]);
            assert_eq!(pointer.rgba.len(), 1024);
            if let Some(frame) = session.take_frame() {
                assert!(
                    frame.pixels.iter().all(|&p| p == 0x0c2238),
                    "remote cursor must not be burned into the desktop image"
                );
            }
            while received.try_recv().is_ok() {}
            let mut latency = Vec::new();
            for x in 20..52 {
                let start = std::time::Instant::now();
                session.send(Input::Pointer {
                    x,
                    y: 100,
                    button: None,
                    down: false,
                    wheel: 0,
                })?;
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        session.poll();
                        while let Ok(event) = received.try_recv() {
                            if event == format!("Move {{ x: {x}, y: 100 }}") {
                                return;
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                })
                .await?;
                latency.push(start.elapsed());
            }
            latency.sort();
            println!(
                "RDP mouse, no graphics: 32 moves; median {:?}, p95 {:?}, max {:?}",
                latency[16], latency[30], latency[31]
            );
            anyhow::ensure!(
                latency[30] < Duration::from_millis(150),
                "loopback input is waiting behind transport/graphics: {:?}",
                latency[30]
            );
            drop(session);
            let mut server = server;
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.0).await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
}
async fn exercise_desktop(remotefx: bool) -> Result<()> {
    let Fixture {
        connection,
        server,
        mut received,
        _keys,
        ..
    } = Fixture::start(remotefx).await?;
    let mut session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    let mut last_frame = None;
    let mut desktop = Canvas::default();
    // RemoteFX performs lossy RGB/YUV conversion; the uncompressed test requires
    // exact pixels, while the codec test verifies every pixel within its tolerance.
    let expected_pixel = |pixel: &u32| {
        [16, 8, 0]
            .into_iter()
            .zip([12_u8, 34, 56])
            .all(|(shift, expected)| {
                ((pixel >> shift) as u8).abs_diff(expected) <= if remotefx { 4 } else { 0 }
            })
    };
    let frame = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            session.poll();
            if let Status::Failed(error) = session.status() {
                anyhow::bail!("{error}");
            }
            if let Some(frame) = session.take_frame() {
                desktop.apply(&frame);
                last_frame = Some((
                    frame.width,
                    frame.height,
                    frame.pixels.first().copied(),
                    frame
                        .pixels
                        .iter()
                        .filter(|pixel| expected_pixel(pixel))
                        .count(),
                    frame
                        .pixels
                        .iter()
                        .enumerate()
                        .find(|(_, pixel)| !expected_pixel(pixel))
                        .map(|(index, pixel)| (index, *pixel)),
                ));
                if desktop.pixels.iter().all(expected_pixel) {
                    return Ok::<_, anyhow::Error>(frame);
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .with_context(|| {
        format!(
            "No complete RDP frame: status={:?}, last={last_frame:?}, server_finished={}",
            session.status(),
            server.0.is_finished()
        )
    })??;
    assert_eq!((frame.width, frame.height), (320, 256));
    assert_eq!(session.status(), Status::Ready);
    for event in [
        Input::ScanCode {
            code: 0x48,
            extended: true,
            down: true,
        },
        Input::ScanCode {
            code: 0x48,
            extended: true,
            down: false,
        },
        Input::Unicode {
            value: 0x4e2d,
            down: true,
        },
        Input::Unicode {
            value: 0x4e2d,
            down: false,
        },
        Input::Pointer {
            x: 123,
            y: 45,
            button: None,
            down: false,
            wheel: 0,
        },
        Input::Pointer {
            x: 123,
            y: 45,
            button: Some(1),
            down: true,
            wheel: 0,
        },
        Input::Pointer {
            x: 123,
            y: 45,
            button: Some(1),
            down: false,
            wheel: 0,
        },
        Input::Pointer {
            x: 123,
            y: 45,
            button: None,
            down: false,
            wheel: -120,
        },
    ] {
        session.send(event)?;
    }
    let mut observed = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !observed
            .iter()
            .any(|event: &String| event.contains("VerticalScroll { value: -120 }"))
        {
            session.poll();
            while let Ok(event) = received.try_recv() {
                observed.push(event);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await?;
    for expected in [
        "Pressed { code: 72, extended: true }",
        "Released { code: 72, extended: true }",
        "UnicodePressed(20013)",
        "UnicodeReleased(20013)",
        "Move { x: 123, y: 45 }",
        "LeftPressed",
        "LeftReleased",
    ] {
        assert!(
            observed.iter().any(|event| event == expected),
            "Missing {expected}: {observed:?}"
        );
    }
    drop(session);
    // Cancellation releases the transport rather than leaving a server-side session.
    let mut task = server;
    let _ = tokio::time::timeout(Duration::from_secs(5), &mut task.0).await?;
    Ok(())
}

#[derive(Default)]
struct Canvas {
    size: [u16; 2],
    pixels: Vec<u32>,
}
impl Canvas {
    fn apply(&mut self, frame: &tomato_services::remote_desktop::Frame) {
        if self.size != [frame.width, frame.height] {
            self.size = [frame.width, frame.height];
            self.pixels = vec![0; usize::from(frame.width) * usize::from(frame.height)];
        }
        let region = frame
            .region
            .unwrap_or(tomato_services::remote_desktop::Region {
                x: 0,
                y: 0,
                width: frame.width,
                height: frame.height,
            });
        assert_eq!(
            frame.pixels.len(),
            usize::from(region.width) * usize::from(region.height)
        );
        for (row, pixels) in frame
            .pixels
            .chunks_exact(usize::from(region.width))
            .enumerate()
        {
            let start =
                (usize::from(region.y) + row) * usize::from(frame.width) + usize::from(region.x);
            self.pixels[start..start + pixels.len()].copy_from_slice(pixels);
        }
    }
}
struct StreamingDisplay(Option<mpsc::UnboundedReceiver<DisplayUpdate>>);
struct StreamingUpdates(mpsc::UnboundedReceiver<DisplayUpdate>);
#[async_trait::async_trait]
impl RdpServerDisplay for StreamingDisplay {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: 1920,
            height: 1080,
        }
    }
    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(StreamingUpdates(
            self.0.take().context("display already connected")?,
        )))
    }
}
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for StreamingUpdates {
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        Ok(self.0.recv().await)
    }
}
fn strip(y: u16, height: u16, pixel: [u8; 4]) -> DisplayUpdate {
    DisplayUpdate::Bitmap(BitmapUpdate {
        x: 0,
        y,
        width: NonZeroU16::new(1920).unwrap(),
        height: NonZeroU16::new(height).unwrap(),
        format: PixelFormat::BgrX32,
        data: pixel.repeat(1920 * usize::from(height)).into(),
        stride: NonZeroUsize::new(1920 * 4).unwrap(),
    })
}
#[tokio::test]
async fn native_rdp_1080p_scanline_burst_coalesces_preserves_every_row_and_input() -> Result<()> {
    tokio::task::LocalSet::new().run_until(async {
        let (send,updates)=mpsc::unbounded_channel();
        let Fixture{connection,mut received,server,_keys,..}=Fixture::with_display(false,StreamingDisplay(Some(updates))).await?;
        send.send(strip(0,1080,[56,34,12,0]))?;
        let mut session=DesktopSession::start(&connection,&tokio::runtime::Handle::current())?;
        let mut desktop=Canvas::default();
        tokio::time::timeout(Duration::from_secs(20),async {
            loop {
                session.poll();
                if let Status::Failed(error)=session.status(){anyhow::bail!("{error}");}
                if let Some(frame)=session.take_frame() {desktop.apply(&frame);if desktop.pixels.iter().all(|pixel|*pixel==0x0c2238) {break;}}
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await??;
        let started=std::time::Instant::now();
        for index in 0..540_u16 {send.send(strip(index*2,2,[77,88,(index%200+20) as u8,0]))?;}
        session.send(Input::Pointer{x:123,y:45,button:None,down:false,wheel:0})?;
        let mut snapshots=0;let mut pixels=0;let mut max_poll=Duration::ZERO;let mut input_seen=false;
        tokio::time::timeout(Duration::from_secs(15),async {
            loop {
                let before=std::time::Instant::now();session.poll();max_poll=max_poll.max(before.elapsed());
                if let Status::Failed(error)=session.status(){anyhow::bail!("{error}");}
                if let Some(frame)=session.take_frame() {snapshots+=1;pixels+=frame.pixels.len();desktop.apply(&frame);}
                while let Ok(event)=received.try_recv(){input_seen|=event=="Move { x: 123, y: 45 }";}
                let complete=(0..1080_usize).all(|row|desktop.pixels[row*1920]==(((row/2%200+20) as u32)<<16|88<<8|77));
                if complete && input_seen {break;}
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await??;
        for row in 0..1080_usize {let expected=((row/2%200+20) as u32)<<16|88<<8|77;anyhow::ensure!(desktop.pixels[row*1920..(row+1)*1920].iter().all(|pixel|*pixel==expected),"stripe pixels lost in row {row}");}
        println!("RDP 1080p: 540 strips, {snapshots} UI snapshots, {pixels} copied pixels, elapsed {:?}, max GUI poll {:?}, input delivered={input_seen}",started.elapsed(),max_poll);
        anyhow::ensure!(snapshots<135,"one screen copy per strip returned");
        anyhow::ensure!(pixels<1920*1080*24,"too many full-screen equivalents");
        drop(session);let mut task=server;let _=tokio::time::timeout(Duration::from_secs(5),&mut task.0).await?;
        Ok::<_,anyhow::Error>(())
    }).await
}

fn square(x: u16, y: u16, color: [u8; 4]) -> DisplayUpdate {
    DisplayUpdate::Bitmap(BitmapUpdate {
        x,
        y,
        width: NonZeroU16::new(32).unwrap(),
        height: NonZeroU16::new(32).unwrap(),
        format: PixelFormat::BgrX32,
        data: color.repeat(1024).into(),
        stride: NonZeroUsize::new(128).unwrap(),
    })
}
#[tokio::test]
async fn native_rdp_sparse_rgba_upload_preserves_distant_damage_and_input() -> Result<()> {
    tokio::task::LocalSet::new().run_until(async {
        let (send,updates)=mpsc::unbounded_channel();
        let Fixture{connection,mut received,server,_keys,..}=Fixture::with_display(false,StreamingDisplay(Some(updates))).await?;
        send.send(strip(0,1080,[56,34,12,0]))?;
        let mut session=DesktopSession::start(&connection,&tokio::runtime::Handle::current())?;
        tokio::time::timeout(Duration::from_secs(20),async {
            loop { session.poll();
                if let Status::Failed(error)=session.status(){anyhow::bail!("{error}");}
                if !session.take_rdp_updates().is_empty(){break;}
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await??;
        // Both changes arrive before the UI reads; unrelated pixels must not be uploaded.
        send.send(square(0,0,[51,34,17,0]))?;
        send.send(square(1888,1048,[102,85,68,0]))?;
        session.send(Input::Pointer{x:123,y:45,button:None,down:false,wheel:0})?;
        let mut pixels=0; let mut patches=0; let mut first=false;let mut last=false;let mut input=false;
        tokio::time::timeout(Duration::from_secs(10),async {
            loop {
                session.poll();
                if let Status::Failed(error)=session.status(){anyhow::bail!("{error}");}
                for update in session.take_rdp_updates() {
                    pixels+=update.frame.pixels.len();patches+=1;
                    anyhow::ensure!((update.frame.width,update.frame.height)==(1920,1080),"canvas dimensions changed");
                    anyhow::ensure!(update.frame.pixels.len()==1024,"distant damage expanded to a bounding screen");
                    let r=update.region.context("sparse update became full")?;
                    let expected=if r.x==0 {first=true;[17,34,51,255]}else{last=true;[68,85,102,255]};
                    anyhow::ensure!(update.frame.pixels.iter().all(|p|p.to_ne_bytes()==expected),"RGBA channel/alpha mismatch");
                }
                while let Ok(event)=received.try_recv(){input|=event=="Move { x: 123, y: 45 }";}
                if first&&last&&input{break;}
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await??;
        assert_eq!(pixels,2048);assert_eq!(patches,2);
        println!("RDP 1080p distant corners: {patches} native RGBA patches, {pixels} uploaded pixels vs 2073600 bounding pixels; input delivered={input}");
        drop(session);let mut task=server;let _=tokio::time::timeout(Duration::from_secs(5),&mut task.0).await?;
        Ok::<_,anyhow::Error>(())
    }).await
}

/// A stalled/minimized GUI must not stop negotiation, decoding, input or cleanup.
#[tokio::test]
async fn native_rdp_progresses_and_sends_input_without_any_gui_poll() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let Fixture {
                connection,
                mut received,
                server,
                _keys,
                ..
            } = Fixture::start(true).await?;
            let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
            tokio::time::timeout(Duration::from_secs(20), async {
                while session.status() != Status::Ready {
                    if let Status::Failed(error) = session.status() {
                        anyhow::bail!("{error}");
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            // Deliberately do not poll the session or consume its image for a while.
            tokio::time::sleep(Duration::from_millis(300)).await;
            let started = std::time::Instant::now();
            session.send(Input::Pointer {
                x: 77,
                y: 123,
                button: None,
                down: false,
                wheel: 0,
            })?;
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let event = received.recv().await.context("RDP input channel closed")?;
                    if event == "Move { x: 77, y: 123 }" {
                        break;
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            println!(
                "RDP without GUI polling: negotiated, decoded, mouse delivered in {:?}",
                started.elapsed()
            );
            let frame = session
                .take_frame()
                .context("background decode did not publish pixels")?;
            anyhow::ensure!(
                frame.pixels.iter().all(|pixel| [16, 8, 0]
                    .into_iter()
                    .zip([12_u8, 34, 56])
                    .all(|(shift, expected)| ((pixel >> shift) as u8).abs_diff(expected) <= 4)),
                "background RemoteFX colors changed"
            );
            drop(session);
            let mut server = server;
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut server.0).await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
}

// Bounded, precisely paced synthetic source. Source timing is measured separately
// from the application's texture-update timing; no file I/O runs per frame.
struct AnimatedDisplay;
struct PacedUpdates {
    receiver: Option<mpsc::Receiver<DisplayUpdate>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for PacedUpdates {
    fn drop(&mut self) {
        // Close before joining, including a producer blocked by the bounded queue.
        self.receiver.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[async_trait::async_trait]
impl RdpServerDisplay for AnimatedDisplay {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: 1920,
            height: 1080,
        }
    }
    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        let (send, receive) = mpsc::channel(4);
        let full_motion = matches!(
            std::env::var("TOMATO_RDP_CAPTURE_PATTERN").as_deref(),
            Ok("full" | "mosaic")
        );
        let worker = std::thread::spawn(move || {
            if send.blocking_send(strip(0, 1080, [56, 34, 12, 0])).is_err() {
                return;
            }
            let start = std::time::Instant::now();
            let mut last = start;
            let mut max_gap = Duration::ZERO;
            let mut frames = 0;
            let mut gaps = Vec::with_capacity(1200);
            for index in 0..1200u16 {
                let deadline = start + Duration::from_nanos(u64::from(index) * 16_666_667);
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    if remaining > Duration::from_millis(16) {
                        std::thread::sleep(remaining - Duration::from_millis(16));
                    } else {
                        // SwitchToThread can defer a Windows source for an entire
                        // scheduler quantum. This test-only final deadline wait
                        // keeps source jitter separate from client presentation.
                        std::hint::spin_loop();
                    }
                }
                if full_motion {
                    if send
                        .blocking_send(strip(0, 1080, [56, 34, (index % 200 + 20) as u8, 0]))
                        .is_err()
                    {
                        break;
                    }
                } else {
                    if send
                        .blocking_send(square(0, 0, [51, 34, (index % 200 + 20) as u8, 0]))
                        .is_err()
                    {
                        break;
                    }
                    if send
                        .blocking_send(square(1888, 1048, [102, 85, (index % 200 + 20) as u8, 0]))
                        .is_err()
                    {
                        break;
                    }
                }
                let now = std::time::Instant::now();
                if frames > 0 {
                    max_gap = max_gap.max(now - last);
                    gaps.push(now - last);
                }
                last = now;
                frames += 1;
            }
            gaps.sort_unstable();
            let p95 = gaps.get(gaps.len() * 95 / 100).copied().unwrap_or_default();
            let bursts = gaps
                .iter()
                .filter(|gap| **gap < Duration::from_millis(4))
                .count();
            eprintln!(
                "RDP synthetic source: frames={frames}, seconds={:.6}, max gap ms={:.3}; p95 gap ms={:.3}; bursts below 4ms={bursts}",
                last.duration_since(start).as_secs_f64(),
                max_gap.as_secs_f64() * 1000.,
                p95.as_secs_f64() * 1000.
            );
        });
        Ok(Box::new(PacedUpdates {
            receiver: Some(receive),
            worker: Some(worker),
        }))
    }
}
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for PacedUpdates {
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        Ok(self
            .receiver
            .as_mut()
            .context("closed source")?
            .recv()
            .await)
    }
}
