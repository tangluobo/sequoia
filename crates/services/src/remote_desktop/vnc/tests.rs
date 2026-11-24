use super::*;
use tokio::net::TcpListener;

#[test]
fn vnc_des_matches_the_original_java_client_including_latin1_replacement() {
    let challenge = std::array::from_fn(|i| i as u8);
    let hex = |bytes: [u8; 16]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    // Produced by the original VncClient.encryptChallenge with Java DES/ECB.
    assert_eq!(
        hex(encrypt_challenge(challenge, "fixture")),
        "b6cdfeac10a6a456b5d53a1644a7a475"
    );
    assert_eq!(
        hex(encrypt_challenge(challenge, "päss🦀")),
        "8a1657bdefbb13d0d298632a6b347c70"
    );
}
#[test]
fn vnc_keys_buttons_and_supplementary_unicode_are_encoded_without_stuck_keys() {
    let mut encoder = input::Encoder::default();
    let a = encoder.encode(Input::Unicode {
        value: 0x4e2d,
        down: true,
    });
    assert_eq!(a, [4, 1, 0, 0, 1, 0, 0x4e, 0x2d]);
    assert!(
        encoder
            .encode(Input::Unicode {
                value: 0xd83e,
                down: true
            })
            .is_empty()
    );
    assert!(
        encoder
            .encode(Input::Unicode {
                value: 0xd83e,
                down: false
            })
            .is_empty()
    );
    assert_eq!(
        encoder.encode(Input::Unicode {
            value: 0xdd80,
            down: true
        }),
        [4, 1, 0, 0, 1, 1, 0xf9, 0x80]
    );
    assert_eq!(
        encoder.encode(Input::Unicode {
            value: 0xdd80,
            down: false
        }),
        [4, 0, 0, 0, 1, 1, 0xf9, 0x80]
    );
    assert_eq!(
        encoder.encode(Input::ScanCode {
            code: 0x1d,
            extended: false,
            down: false
        }),
        [4, 0, 0, 0, 0, 0, 0xff, 0xe3]
    );
    assert_eq!(
        encoder.encode(Input::Pointer {
            x: 3,
            y: 4,
            button: Some(2),
            down: true,
            wheel: 0
        }),
        [5, 4, 0, 3, 0, 4]
    );
    assert_eq!(
        encoder.encode(Input::Pointer {
            x: 3,
            y: 4,
            button: None,
            down: false,
            wheel: 120
        }),
        [5, 12, 0, 3, 0, 4, 5, 4, 0, 3, 0, 4]
    );
    assert_eq!(
        encoder.encode(Input::Pointer {
            x: 3,
            y: 4,
            button: Some(2),
            down: false,
            wheel: 0
        }),
        [5, 0, 0, 3, 0, 4]
    );
}
async fn server_init(socket: &mut TcpStream, minor: u8, auth: bool) -> Result<()> {
    server_init_size(socket, minor, auth, 2, 1).await
}
async fn server_init_size(
    socket: &mut TcpStream,
    minor: u8,
    auth: bool,
    width: u16,
    height: u16,
) -> Result<()> {
    socket
        .write_all(format!("RFB 003.{minor:03}\n").as_bytes())
        .await?;
    let mut banner = [0; 12];
    socket.read_exact(&mut banner).await?;
    ensure!(
        banner == format!("RFB 003.{minor:03}\n").as_bytes(),
        "wrong protocol version"
    );
    if minor == 3 {
        socket.write_u32(if auth { 2 } else { 1 }).await?;
    } else {
        socket.write_all(&[1, if auth { 2 } else { 1 }]).await?;
        ensure!(
            socket.read_u8().await? == if auth { 2 } else { 1 },
            "wrong authentication selection"
        );
    }
    if auth {
        socket.write_all(&(0u8..16).collect::<Vec<_>>()).await?;
        let mut response = [0; 16];
        socket.read_exact(&mut response).await?;
        ensure!(
            response
                == [
                    0xb6, 0xcd, 0xfe, 0xac, 0x10, 0xa6, 0xa4, 0x56, 0xb5, 0xd5, 0x3a, 0x16, 0x44,
                    0xa7, 0xa4, 0x75
                ],
            "wrong Java-compatible response"
        );
    }
    if auth || minor >= 8 {
        socket.write_u32(0).await?;
    }
    ensure!(socket.read_u8().await? == 1, "desktop must be shared");
    socket.write_u16(width).await?;
    socket.write_u16(height).await?;
    socket.write_all(&[0; 16]).await?;
    socket.write_u32(7).await?;
    socket.write_all(b"fixture").await?;
    let mut format = [0; 20];
    socket.read_exact(&mut format).await?;
    ensure!(
        format
            == [
                0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0
            ],
        "wrong pixel format"
    );
    let mut encodings = [0; 16];
    socket.read_exact(&mut encodings).await?;
    ensure!(
        encodings == [2, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 0, 255, 255, 255, 33],
        "wrong advertised encodings"
    );
    let mut request = [0; 10];
    socket.read_exact(&mut request).await?;
    ensure!(
        request
            == [
                3,
                0,
                0,
                0,
                0,
                0,
                (width >> 8) as u8,
                width as u8,
                (height >> 8) as u8,
                height as u8
            ],
        "missing initial update"
    );
    Ok(())
}
async fn wait_frame(session: &DesktopSession, expected: &[u32]) -> Result<()> {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(frame) = session.take_frame()
                && frame.pixels == expected
            {
                return Ok(());
            }
            if let Status::Failed(message) = session.status() {
                bail!("{message}");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn native_vnc_versions_raw_hextile_resize_clipboard_input_and_drop() -> Result<()> {
    for minor in [3, 7, 8] {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut connection = Connection::new("VNC");
        connection.set("host", "127.0.0.1");
        connection.set("port", listener.local_addr()?.port());
        connection.set("password", if minor == 8 { "fixture" } else { "" });
        connection.set("mapClipboard", minor != 7);
        let (fragment_tx, fragment_rx) = tokio::sync::oneshot::channel();
        let (continue_tx, continue_rx) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            server_init(&mut socket, minor, minor == 8).await?;
            // Fragment a Raw rectangle, then require input before finishing it.
            socket
                .write_all(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 0, 0x33, 0x22])
                .await?;
            let _ = fragment_tx.send(());
            let mut key = [0; 8];
            socket.read_exact(&mut key).await?;
            ensure!(
                key == [4, 1, 0, 0, 0, 0, 0, 65],
                "input lost during partial rectangle"
            );
            socket.write_all(&[0x11, 0, 0x66, 0x55, 0x44, 0]).await?;
            let mut request = [0; 10];
            socket.read_exact(&mut request).await?;
            ensure!(request[0..2] == [3, 1], "missing incremental update");
            continue_rx.await?;
            // Hextile: background + foreground + one monochrome subrectangle.
            socket
                .write_all(&[
                    0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 5, 14, 0x33, 0x22, 0x11, 0, 0, 0,
                    0xff, 0, 1, 0x10, 0,
                ])
                .await?;
            socket.read_exact(&mut request).await?;
            // Resize followed by Raw in the same framebuffer update.
            socket
                .write_all(&[
                    0, 0, 0, 2, 0, 0, 0, 0, 0, 3, 0, 1, 255, 255, 255, 33, 0, 0, 0, 0, 0, 3, 0, 1,
                    0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0,
                ])
                .await?;
            socket.read_exact(&mut request).await?;
            ensure!(
                request[6..10] == [0, 3, 0, 1],
                "resize request retained stale dimensions"
            );
            if minor != 7 {
                let mut cut = [0; 12];
                socket.read_exact(&mut cut).await?;
                ensure!(
                    cut == [6, 0, 0, 0, 0, 0, 0, 4, b'c', b'a', b'f', 0xe9],
                    "clipboard encoding mismatch"
                );
            }
            socket
                .write_all(&[3, 0, 0, 0, 0, 0, 0, 4, b'c', b'a', b'f', 0xe9, 2])
                .await?;
            let mut byte = [0];
            ensure!(
                socket.read(&mut byte).await? == 0,
                "closing tab did not close socket"
            );
            Ok::<_, anyhow::Error>(())
        });
        let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
        timeout(Duration::from_secs(5), fragment_rx).await??;
        assert_eq!(session.status(), Status::Ready);
        assert_eq!(session.desktop_name(), "fixture");
        session.send(Input::Unicode {
            value: 65,
            down: true,
        })?;
        wait_frame(&session, &[0x112233, 0x445566]).await?;
        let _ = continue_tx.send(());
        wait_frame(&session, &[1, 2, 3]).await?;
        if minor != 7 {
            session.send_clipboard("café".into())?;
        }
        let mut received_clipboard = None;
        timeout(Duration::from_secs(5), async {
            loop {
                if let Some(text) = session.take_clipboard() {
                    received_clipboard = Some(text);
                }
                if session.take_bell() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        assert_eq!(
            received_clipboard.as_deref(),
            if minor == 7 { None } else { Some("café") }
        );
        // No GUI polling was required, including for the RAII abort guard.
        drop(session);
        timeout(Duration::from_secs(5), peer).await???;
    }
    Ok(())
}
#[tokio::test]
async fn vnc_rejects_out_of_bounds_hextile_and_unbounded_framebuffers() -> Result<()> {
    assert!(framebuffer::Screen::new(65535, 65535).is_err());
    let mut screen = framebuffer::Screen::new(2, 1)?;
    let mut valid: &[u8] = &[
        0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 5, 14, 0x33, 0x22, 0x11, 0, 0, 0, 0xff, 0, 1,
        0x10, 0,
    ];
    match screen.read_message(&mut valid).await? {
        framebuffer::Message::Frame(frame) => assert_eq!(frame.pixels, [0x112233, 0xff0000]),
        _ => bail!("Hextile did not publish a frame"),
    }
    assert!(matches!(
        screen.read_message(&mut valid).await?,
        framebuffer::Message::Closed
    ));
    let mut truncated: &[u8] = &[0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 0, 1];
    assert!(screen.read_message(&mut truncated).await.is_err());
    let mut data: &[u8] = &[
        0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 5, 14, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0x10, 0x10,
    ];
    assert!(
        screen
            .read_message(&mut data)
            .await
            .err()
            .expect("malformed rectangle must fail")
            .to_string()
            .contains("越界")
    );
    Ok(())
}
#[tokio::test]
async fn closing_vnc_before_handshake_closes_the_owned_socket() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut connection = Connection::new("VNC");
    connection.set("host", "127.0.0.1");
    connection.set("port", listener.local_addr()?.port());
    let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept()).await??;
    drop(session);
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(5), socket.read(&mut byte)).await??,
        0
    );
    Ok(())
}

#[tokio::test]
async fn vnc_reports_rejected_authentication_without_saving_credentials() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut connection = Connection::new("VNC");
    connection.set("host", "127.0.0.1");
    connection.set("port", listener.local_addr()?.port());
    connection.set("password", "incorrect-fixture");
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        socket.write_all(b"RFB 003.008\n").await?;
        let mut banner = [0; 12];
        socket.read_exact(&mut banner).await?;
        socket.write_all(&[1, 2]).await?;
        ensure!(
            socket.read_u8().await? == 2,
            "wrong authentication selection"
        );
        socket.write_all(&[0; 16]).await?;
        let mut response = [0; 16];
        socket.read_exact(&mut response).await?;
        socket.write_u32(1).await?;
        socket.write_u32(6).await?;
        socket.write_all("拒绝".as_bytes()).await?;
        Ok::<_, anyhow::Error>(())
    });
    let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    timeout(Duration::from_secs(5), async {
        loop {
            if let Status::Failed(message) = session.status() {
                ensure!(message.contains("拒绝"), "{message}");
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await??;
    peer.await??;
    Ok(())
}
async fn wait_status(session: &DesktopSession, expected: Status) -> Result<()> {
    timeout(Duration::from_secs(5), async {
        loop {
            let status = session.status();
            if status == expected {
                return Ok(());
            }
            if let Status::Failed(message) = status {
                bail!("{message}");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn vnc_password_prompt_resumes_the_original_socket_and_cancel_releases_it() -> Result<()> {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut connection = Connection::new("VNC");
        connection.set("host", "127.0.0.1");
        connection.set("port", listener.local_addr()?.port());
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let result = server_init(&mut socket, 8, true).await;
            if cancel {
                ensure!(
                    result
                        .err()
                        .and_then(|e| e.downcast::<std::io::Error>().ok())
                        .is_some(),
                    "cancel must close the original authentication socket"
                );
            } else {
                result?;
                let mut byte = [0];
                ensure!(
                    socket.read(&mut byte).await? == 0,
                    "session did not close its socket"
                );
            }
            Ok::<_, anyhow::Error>(())
        });
        let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
        wait_status(&session, Status::PasswordRequired).await?;
        if cancel {
            session.cancel_password()?;
            ensure!(session.provide_password("fixture".into()).is_err());
            timeout(Duration::from_secs(5), peer).await???;
            wait_status(&session, Status::Failed("VNC 服务器需要密码".into())).await?;
        } else {
            session.provide_password("fixture".into())?;
            wait_status(&session, Status::Ready).await?;
            drop(session);
            timeout(Duration::from_secs(5), peer).await???;
        }
    }
    Ok(())
}
#[tokio::test]
async fn vnc_large_clipboard_is_streamed_while_frames_arrive_and_pastes_cannot_overwrite()
-> Result<()> {
    assert_eq!(clipboard_length("é🦀")?, 2);
    assert!(clipboard_length(&"x".repeat(16 * 1024 * 1024 + 1)).is_err());
    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.set_recv_buffer_size(64 * 1024)?;
    socket.bind("127.0.0.1:0".parse()?)?;
    let listener = socket.listen(16)?;
    let mut connection = Connection::new("VNC");
    connection.set("host", "127.0.0.1");
    connection.set("port", listener.local_addr()?.port());
    connection.set("password", "fixture");
    let (drain_tx, drain_rx) = tokio::sync::oneshot::channel();
    let (keys_tx, keys_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        server_init(&mut socket, 8, true).await?;
        let mut header = [0; 8];
        socket.read_exact(&mut header).await?;
        ensure!(
            header[..4] == [6, 0, 0, 0]
                && u32::from_be_bytes(header[4..].try_into()?) == 16 * 1024 * 1024,
            "clipboard length must match the original 16 MiB limit"
        );
        socket
            .write_all(&[
                0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 0, 0x33, 0x22, 0x11, 0, 0x66, 0x55,
                0x44, 0,
            ])
            .await?;
        // The reader must progress while a full-size clipboard write is blocked.
        drain_rx.await?;
        let mut remaining = 16 * 1024 * 1024;
        let mut buffer = [0; 65536];
        while remaining > 0 {
            let size = remaining.min(buffer.len());
            socket.read_exact(&mut buffer[..size]).await?;
            ensure!(
                buffer[..size].iter().all(|b| *b == b'x'),
                "clipboard was overwritten"
            );
            remaining -= size;
        }
        let mut keys = Vec::new();
        while keys.len() < 4 {
            match socket.read_u8().await? {
                3 => {
                    let mut rest = [0; 9];
                    socket.read_exact(&mut rest).await?;
                }
                4 => {
                    let mut rest = [0; 7];
                    socket.read_exact(&mut rest).await?;
                    keys.push((rest[0], u32::from_be_bytes(rest[3..].try_into()?)));
                }
                other => bail!("unexpected client message {other}"),
            }
        }
        ensure!(
            keys == [(1, 0xffe3), (1, 0x76), (0, 0x76), (0, 0xffe3)],
            "clipboard and Ctrl+V were reordered"
        );
        let _ = keys_tx.send(());
        // A coalesced framebuffer request can still be waiting behind the key-up.
        loop {
            let mut byte = [0];
            if socket.read(&mut byte).await? == 0 {
                break;
            }
            ensure!(byte[0] == 3, "unexpected trailing client message");
            let mut rest = [0; 9];
            socket.read_exact(&mut rest).await?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let session = DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
    wait_status(&session, Status::Ready).await?;
    session.prepare_paste("x".repeat(16 * 1024 * 1024))?;
    assert!(session.prepare_paste("must-not-overwrite".into()).is_err());
    for (code, down) in [(0x1d, true), (0x2f, true), (0x2f, false), (0x1d, false)] {
        session.send(Input::ScanCode {
            code,
            extended: false,
            down,
        })?;
    }
    wait_frame(&session, &[0x112233, 0x445566]).await?;

    let _ = drain_tx.send(());
    timeout(Duration::from_secs(10), keys_rx).await??;
    drop(session);
    timeout(Duration::from_secs(5), peer).await???;
    Ok(())
}
/// Development-only capture of the original Java view and the native Rust view
/// against the same synthetic desktop. Never opens a stored host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires built GUI, the isolated compiled Java oracle, and an active desktop"]
async fn vnc_original_and_native_window_capture() -> Result<()> {
    use std::{fs, path::Path, process::Command};
    // Keep a regular Windows drive path: canonicalize produces a verbatim UNC
    // prefix which the Java classpath parser does not interpret as a drive.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target");
    let java =
        std::env::var_os("TOMATO_ORIGINAL_VNC_JAVA").context("set the JDK 25 java executable")?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let mut connection = Connection::new("VNC");
    connection.name = "VNC 回环桌面".into();
    connection.set("host", "127.0.0.1");
    connection.set("port", port);
    connection.set("username", "fixture");
    connection.set("password", "fixture");
    let data = tempfile::tempdir()?;
    fs::write(
        data.path().join("connections.json"),
        serde_json::to_vec(&vec![connection])?,
    )?;
    let peer = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await?;
            server_init_size(&mut socket, 8, true, 640, 400).await?;
            let mut packet = vec![0, 0, 0, 2];
            for encoding in [-223i32, 0] {
                packet.extend_from_slice(&[0, 0, 0, 0, 2, 128, 1, 144]);
                packet.extend_from_slice(&encoding.to_be_bytes());
            }
            for y in 0..400u32 {
                for x in 0..640u32 {
                    let pixel = if x % 80 < 2 || y % 80 < 2 {
                        0xeeeeee
                    } else {
                        ((x / 80) * 28 + 24) << 16 | ((y / 80) * 40 + 24) << 8 | 80
                    };
                    packet.extend_from_slice(&pixel.to_le_bytes());
                }
            }
            socket.write_all(&packet).await?;
            let mut drain = [0; 1024];
            while socket.read(&mut drain).await? != 0 {}
        }
        Ok::<_, anyhow::Error>(())
    });
    let classpath = fs::read_to_string(target.join("original-ui-oracle/classpath.txt"))?;
    let classes = target
        .join("original-ui-oracle/classes")
        .to_string_lossy()
        .replace('\\', "/");
    let image = target
        .join("ui-vnc-original-pane.png")
        .to_string_lossy()
        .replace('\\', "/");
    let args = target.join("original-ui-oracle/vnc-capture.args");
    fs::write(
        &args,
        format!(
            "-cp\n\"{classes};{}\"\ncom.tangluobo.tomato.module.connect.handler.VncPaneCapture\n\"{image}\"\n{port}\n",
            classpath.trim().replace('\\', "/")
        ),
    )?;
    let output = Command::new(java)
        .arg(format!("@{}", args.display()))
        .output()?;
    fs::write(target.join("validation-java-vnc-pane.log"), &output.stderr)?;
    ensure!(
        output.status.success(),
        "Java VNC capture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let executable = std::env::var_os("TOMATO_VNC_CAPTURE_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            target
                .join("debug")
                .join(format!("tomato{}", std::env::consts::EXE_SUFFIX))
        });
    let output = Command::new(executable)
        .env("TOMATO_DATA_DIR", data.path())
        .env("TOMATO_UI_PREVIEW", "vnc")
        .env(
            "TOMATO_UI_CAPTURE",
            target.join("ui-vnc-workspace-native.png"),
        )
        .output()?;
    fs::write(
        target.join("validation-native-vnc-pane.log"),
        &output.stderr,
    )?;
    ensure!(
        output.status.success(),
        "Rust VNC capture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    timeout(Duration::from_secs(5), peer).await???;
    Ok(())
}
