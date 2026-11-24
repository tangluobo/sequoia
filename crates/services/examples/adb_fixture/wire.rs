use super::*;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite, WriteHalf},
    net::TcpStream,
    sync::{Notify, mpsc},
};
use tokio_util::task::AbortOnDropHandle;

const PAYLOAD: usize = 64 * 1024;

struct Packet {
    name: [u8; 4],
    sender: u32,
    receiver: u32,
    data: Vec<u8>,
}
impl Packet {
    async fn read(socket: &mut (impl AsyncRead + Unpin)) -> Result<Self> {
        let mut header = [0; 24];
        socket.read_exact(&mut header).await?;
        let value = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        let size = value(12) as usize;
        ensure!(
            size <= PAYLOAD && value(20) == !value(0),
            "malformed ADB fixture packet"
        );
        let mut data = vec![0; size];
        socket.read_exact(&mut data).await?;
        Ok(Self {
            name: header[..4].try_into()?,
            sender: value(4),
            receiver: value(8),
            data,
        })
    }
}

async fn send(
    socket: &mut (impl AsyncWrite + Unpin),
    name: &[u8; 4],
    sender: u32,
    receiver: u32,
    data: &[u8],
) -> Result<()> {
    let mut packet = Vec::with_capacity(24 + data.len());
    packet.extend_from_slice(name);
    packet.extend_from_slice(&sender.to_le_bytes());
    packet.extend_from_slice(&receiver.to_le_bytes());
    packet.extend_from_slice(&(data.len() as u32).to_le_bytes());
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet.extend_from_slice(&(!u32::from_le_bytes(*name)).to_le_bytes());
    packet.extend_from_slice(data);
    socket.write_all(&packet).await?;
    Ok(())
}

enum Event {
    Packet(Packet),
    Output(u32, Vec<u8>),
    Close(u32),
    Disconnect,
}

struct Channel {
    host: u32,
    input: WriteHalf<DuplexStream>,
    acknowledged: Arc<Notify>,
    _service: AbortOnDropHandle<()>,
    _output: AbortOnDropHandle<()>,
}

enum Service {
    Sync,
    Shell,
    Png,
    Server,
    Video,
    Control,
    Remove,
}
impl Service {
    async fn run(self, files: PathBuf, stream: DuplexStream) -> Result<()> {
        match self {
            Self::Sync => sync(files, stream).await,
            Self::Shell => shell(files, stream).await,
            Self::Png => png(files, stream).await,
            Self::Video => scrcpy::video(files, stream).await,
            Self::Control => scrcpy::control(files, stream).await,
            Self::Server => {
                let _stream = stream;
                std::future::pending().await
            }
            Self::Remove => Ok(()),
        }
    }
}

impl Channel {
    fn open(
        host: u32,
        id: u32,
        service: Service,
        files: PathBuf,
        events: mpsc::Sender<Event>,
    ) -> Self {
        let (stream, peer) = tokio::io::duplex(PAYLOAD);
        let (mut output, input) = tokio::io::split(stream);
        let service = AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = service.run(files, peer).await;
        }));
        let acknowledged = Arc::new(Notify::new());
        let ack = acknowledged.clone();
        let output = AbortOnDropHandle::new(tokio::spawn(async move {
            let mut buffer = vec![0; PAYLOAD];
            while let Ok(size) = output.read(&mut buffer).await {
                if size == 0 {
                    break;
                }
                if events
                    .send(Event::Output(id, buffer[..size].to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
                ack.notified().await;
            }
            let _ = events.send(Event::Close(id)).await;
        }));
        Self {
            host,
            input,
            acknowledged,
            _service: service,
            _output: output,
        }
    }
}

pub async fn serve(mut socket: TcpStream, files: PathBuf) -> Result<()> {
    socket.set_nodelay(true)?;
    let hello = Packet::read(&mut socket).await?;
    ensure!(&hello.name == b"CNXN", "expected ADB handshake");
    log(&files, &["connect"])?;
    send(
        &mut socket,
        b"CNXN",
        0x0100_0001,
        PAYLOAD as u32,
        b"device::features=\0",
    )
    .await?;
    let (mut read, mut write) = socket.into_split();
    let (events, mut inbox) = mpsc::channel(32);
    let incoming = events.clone();
    let _reader = AbortOnDropHandle::new(tokio::spawn(async move {
        while let Ok(packet) = Packet::read(&mut read).await {
            if incoming.send(Event::Packet(packet)).await.is_err() {
                return;
            }
        }
        let _ = incoming.send(Event::Disconnect).await;
    }));

    let mut channels: HashMap<u32, Channel> = HashMap::new();
    let mut next = 0;
    let mut server_socket = String::new();
    let mut video_opened = false;
    while let Some(event) = inbox.recv().await {
        match event {
            Event::Disconnect => break,
            Event::Output(id, data) => {
                if let Some(channel) = channels.get(&id) {
                    send(&mut write, b"WRTE", id, channel.host, &data).await?;
                }
            }
            Event::Close(id) => {
                if let Some(channel) = channels.remove(&id) {
                    send(&mut write, b"CLSE", id, channel.host, &[]).await?;
                }
            }
            Event::Packet(packet) => match &packet.name {
                b"OPEN" => {
                    let name = std::str::from_utf8(&packet.data)?.trim_end_matches('\0');
                    let service = match name {
                        "sync:" if files.with_extension("reject-scrcpy").exists() => {
                            send(&mut write, b"CLSE", 0, packet.sender, &[]).await?;
                            continue;
                        }
                        "sync:" => Service::Sync,
                        "exec:sh" => Service::Shell,
                        name if name.contains("screencap -d 0 -p") => {
                            log(&files, &["screencap"])?;
                            Service::Png
                        }
                        name if name.contains("com.genymobile.scrcpy.Server") => {
                            let arguments: Vec<_> = name.split_whitespace().collect();
                            let scid = arguments
                                .iter()
                                .find_map(|arg| arg.strip_prefix("scid="))
                                .context("scrcpy session ID missing")?;
                            server_socket = format!(
                                "localabstract:scrcpy_{:08x}",
                                u32::from_str_radix(scid, 16)?
                            );
                            log(&files, &arguments)?;
                            Service::Server
                        }
                        name if name.starts_with("exec:rm -f /data/local/tmp/tomato-") => {
                            log(&files, &["remove", name.trim_start_matches("exec:rm -f ")])?;
                            std::fs::write(files.with_extension("server-removed"), b"owned")?;
                            Service::Remove
                        }
                        name if name == server_socket && !video_opened => {
                            video_opened = true;
                            Service::Video
                        }
                        name if name == server_socket => Service::Control,
                        _ => {
                            send(&mut write, b"CLSE", 0, packet.sender, &[]).await?;
                            continue;
                        }
                    };
                    next += 1;
                    send(&mut write, b"OKAY", next, packet.sender, &[]).await?;
                    channels.insert(
                        next,
                        Channel::open(packet.sender, next, service, files.clone(), events.clone()),
                    );
                }
                b"WRTE" => {
                    if let Some(channel) = channels.get_mut(&packet.receiver)
                        && channel.input.write_all(&packet.data).await.is_ok()
                    {
                        send(&mut write, b"OKAY", packet.receiver, packet.sender, &[]).await?;
                    } else {
                        channels.remove(&packet.receiver);
                        send(&mut write, b"CLSE", packet.receiver, packet.sender, &[]).await?;
                    }
                }
                b"OKAY" => {
                    if let Some(channel) = channels.get(&packet.receiver) {
                        channel.acknowledged.notify_one();
                    }
                }
                b"CLSE" => {
                    if channels.remove(&packet.receiver).is_some() {
                        send(&mut write, b"CLSE", packet.receiver, packet.sender, &[]).await?;
                    }
                }
                _ => anyhow::bail!("unexpected ADB fixture command"),
            },
        }
    }
    Ok(())
}
