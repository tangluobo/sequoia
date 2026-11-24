use super::*;
use tokio::net::TcpListener;

const VIDEO: &[u8] = include_bytes!("../../../../tests/fixtures/adb/red64x48.scrcpy");
const JAR: &[u8] = include_bytes!("../../../../assets/adb/scrcpy-server-v3.3.4.jar");

#[test]
fn wireless_pairing_validates_the_pairing_port_and_keeps_the_code_private() -> Result<()> {
    use crate::commands::Command;
    let parsed: Command = serde_json::from_value(serde_json::json!({
        "command":"adb_pair",
        "params":{"address":"[::1]:4242","code":"013257"}
    }))?;
    let Command::AdbPair(request) = &parsed else {
        panic!("missing ADB request")
    };
    assert_eq!(request.validate()?, "[::1]:4242");
    assert!(!format!("{parsed:?}").contains("013257"));
    assert!(!serde_json::to_string(&parsed)?.contains("013257"));
    assert!(
        AdbPairing::new("127.0.0.1:0".into(), "013257".into())
            .validate()
            .is_err()
    );
    assert!(
        AdbPairing::new("127.0.0.1:4242".into(), "12345x".into())
            .validate()
            .is_err()
    );
    Ok(())
}

struct Packet {
    name: [u8; 4],
    local: u32,
    remote: u32,
    data: Vec<u8>,
}

async fn receive(socket: &mut TcpStream) -> Result<Packet> {
    let mut name = [0; 4];
    socket.read_exact(&mut name).await?;
    let local = socket.read_u32_le().await?;
    let remote = socket.read_u32_le().await?;
    let size = socket.read_u32_le().await? as usize;
    let _checksum = socket.read_u32_le().await?;
    assert_eq!(socket.read_u32_le().await?, !u32::from_le_bytes(name));
    assert!(size <= PACKET);
    let mut data = vec![0; size];
    socket.read_exact(&mut data).await?;
    Ok(Packet {
        name,
        local,
        remote,
        data,
    })
}

async fn send(
    socket: &mut TcpStream,
    name: &[u8; 4],
    local: u32,
    remote: u32,
    data: &[u8],
) -> Result<()> {
    let mut packet = Vec::with_capacity(24 + data.len());
    packet.extend(name);
    packet.extend(local.to_le_bytes());
    packet.extend(remote.to_le_bytes());
    packet.extend((data.len() as u32).to_le_bytes());
    packet.extend(0u32.to_le_bytes());
    packet.extend((!u32::from_le_bytes(*name)).to_le_bytes());
    packet.extend(data);
    socket.write_all(&packet).await?;
    Ok(())
}

async fn serve(listener: TcpListener, key: Vec<u8>, image: Vec<u8>) -> Result<()> {
    let (mut socket, _) = listener.accept().await?;
    let hello = receive(&mut socket).await?;
    assert_eq!(&hello.name, b"CNXN");
    send(&mut socket, b"AUTH", 1, 0, &[7; 20]).await?;
    let signature = receive(&mut socket).await?;
    assert_eq!(
        (&signature.name, signature.local, signature.data.len()),
        (b"AUTH", 2, 256)
    );
    send(&mut socket, b"AUTH", 1, 0, &[8; 20]).await?;
    let public = receive(&mut socket).await?;
    assert_eq!((&public.name, public.local, public.data), (b"AUTH", 3, key));
    send(
        &mut socket,
        b"CNXN",
        0x0100_0001,
        PACKET as u32,
        b"device::features=\0",
    )
    .await?;

    let mut sync = Vec::new();
    let mut next = 0;
    let mut control = 0;
    let mut video_opened = false;
    let mut server_socket = None;
    let mut remote_jar = String::new();
    loop {
        let packet = receive(&mut socket).await?;
        match &packet.name {
            b"OPEN" => {
                next += 1;
                let service = std::str::from_utf8(&packet.data)?
                    .strip_suffix('\0')
                    .unwrap();
                if let Some(scid) = service.strip_prefix("localabstract:scrcpy_") {
                    assert_eq!(u32::from_str_radix(scid, 16)?, server_socket.unwrap());
                    send(&mut socket, b"OKAY", next, packet.local, &[]).await?;
                    if !video_opened {
                        let mut video = b"\0h264\0\0\0\x40\0\0\0\x30".to_vec();
                        video.extend(VIDEO);
                        send(&mut socket, b"WRTE", next, packet.local, &video).await?;
                        video_opened = true;
                    }
                } else {
                    send(&mut socket, b"OKAY", next, packet.local, &[]).await?;
                    match service {
                        "sync:" => {}
                        "exec:sh" => control = next,
                        s if s.contains("screencap -d 0 -p") => {
                            send(&mut socket, b"WRTE", next, packet.local, &image).await?;
                        }
                        s if s.contains("com.genymobile.scrcpy.Server") => {
                            assert!(s.starts_with(&format!("exec:CLASSPATH={remote_jar} ")));
                            assert!(s.contains(" video_codec=h264 "));
                            let scid = s
                                .split_whitespace()
                                .find_map(|word| word.strip_prefix("scid="))
                                .unwrap();
                            server_socket = Some(u32::from_str_radix(scid, 16)?);
                        }
                        s if s == format!("exec:rm -f {remote_jar}") => {
                            send(&mut socket, b"CLSE", next, packet.local, &[]).await?;
                            return Ok(());
                        }
                        _ => anyhow::bail!("Unexpected ADB service {service}"),
                    }
                }
            }
            b"WRTE" => {
                send(&mut socket, b"OKAY", packet.remote, packet.local, &[]).await?;
                if packet.remote == control {
                    assert!(std::str::from_utf8(&packet.data)?.contains("input 'keyevent' '3'"));
                    send(
                        &mut socket,
                        b"WRTE",
                        packet.remote,
                        packet.local,
                        b"\n__TOMATO_ADB_ACK__:0\n",
                    )
                    .await?;
                    continue;
                }
                sync.extend(packet.data);
                if let Some((path, uploaded)) = uploaded_jar(&sync)? {
                    assert_eq!(uploaded, JAR);
                    remote_jar = path;
                    sync.clear();
                    send(
                        &mut socket,
                        b"WRTE",
                        packet.remote,
                        packet.local,
                        b"OKAY\0\0\0\0",
                    )
                    .await?;
                }
            }
            b"CLSE" => send(&mut socket, b"CLSE", packet.remote, packet.local, &[]).await?,
            b"OKAY" => {}
            _ => anyhow::bail!("Unexpected ADB packet {:?}", packet.name),
        }
    }
}

fn uploaded_jar(mut bytes: &[u8]) -> Result<Option<(String, Vec<u8>)>> {
    debug_assert!(bytes.len() <= JAR.len() + 4096);
    let mut file = String::new();
    let mut jar = Vec::new();
    while bytes.len() >= 8 {
        let value = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
        match &bytes[..4] {
            b"DONE" => return Ok(Some((file, jar))),
            b"QUIT" => return Ok(None),
            _ if value > bytes.len() - 8 => return Ok(None),
            b"SEND" => {
                let (path, _) = std::str::from_utf8(&bytes[8..8 + value])?
                    .rsplit_once(',')
                    .unwrap();
                assert!(path.starts_with("/data/local/tmp/tomato-") && path.ends_with(".jar"));
                file = path.into();
            }
            b"DATA" => jar.extend(&bytes[8..8 + value]),
            _ => anyhow::bail!("Unexpected ADB sync command"),
        }
        bytes = &bytes[8 + value..];
    }
    Ok(None)
}

#[tokio::test]
async fn native_adb_authorizes_and_uses_one_socket_for_png_control_and_scrcpy() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_owned();
    let key = tokio::task::spawn_blocking(move || {
        let name = KeyName::new("Tomato@test")?;
        let key = load_or_create_key(&path, name)?;
        let reused = load_or_create_key(&path, name)?;
        assert_eq!(key.public_key_wire(), reused.public_key_wire());
        Ok::<_, anyhow::Error>(key)
    })
    .await??;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(2, 2).write_to(&mut encoded, image::ImageFormat::Png)?;
    let encoded = encoded.into_inner();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let fixture = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        serve(listener, key.public_key_wire().to_vec(), encoded).await
    }));
    let key = store::load(directory.path(), &mut OsRng, KeyName::new("Tomato@test")?)?;
    timeout(Duration::from_secs(12), async {
        let transport = Transport::authenticate(TcpStream::connect(address).await?, key).await?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (ready, frame) = tokio::sync::oneshot::channel();
        let ready = Mutex::new(Some(ready));
        let notify: Notifier = Arc::new(Mutex::new(Some(Arc::new(move || {
            if let Some(sender) = ready.lock().unwrap().take() {
                let _ = sender.send(());
            }
        }))));
        tokio::select! {
            result = capture(&transport, &shared, &notify) => result?,
            result = frame => result?,
        }
        assert_eq!(
            shared.lock().unwrap().frame.as_ref().unwrap().pixels.len(),
            4
        );
        Control::open(&transport)
            .await?
            .send("'keyevent' '3'")
            .await?;
        let connection = Connection::new("ADB");
        let session = scrcpy::Session::open(&connection, &transport).await?;
        drop(session);
        scrcpy::wait_cleanup().await;
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    timeout(Duration::from_secs(5), fixture).await???;
    Ok(())
}
