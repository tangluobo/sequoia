//! Direct ADB screen and input sessions.
mod native;
pub use native::AdbPairing;
pub(super) mod pool;
pub(super) mod quality;
pub(super) mod scrcpy;
use super::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{
    io::{
        AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
        WriteHalf,
    },
    time::timeout,
};
const MAX_FRAME: usize = 48 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Default)]
pub struct VideoStats {
    pub mode: &'static str,
    pub size: [u16; 2],
    pub frames_per_second: f32,
    pub decoded_frames: u64,
    pub superseded_frames: u64,
    pub queue_resets: u64,
    pub last_decode_micros: u64,
}
fn endpoint(connection: &Connection) -> Result<String> {
    let host = connection.text("host").trim();
    let host = host.strip_prefix("adb://").unwrap_or(host);
    ensure!(!host.is_empty() && !host.starts_with('-'), "ADB 地址无效");
    let port = if connection.number("port") == 0 {
        5555
    } else {
        u16::try_from(connection.number("port"))?
    };
    if host.starts_with('[') && host.contains("]:") {
        return Ok(host.into());
    }
    if host.matches(':').count() == 1
        && host
            .rsplit_once(':')
            .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        return Ok(host.into());
    }
    if host.starts_with('[') && host.ends_with(']') {
        return Ok(format!("{host}:{port}"));
    }
    Ok(if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    })
}
async fn png(reader: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut signature = [0; 8];
    reader.read_exact(&mut signature).await?;
    ensure!(&signature == b"\x89PNG\r\n\x1a\n", "无效的 ADB PNG 数据流");
    let mut bytes = signature.to_vec();
    loop {
        let mut header = [0; 8];
        reader.read_exact(&mut header).await?;
        let size = u32::from_be_bytes(header[..4].try_into()?) as usize;
        ensure!(
            size <= MAX_FRAME && bytes.len().saturating_add(size).saturating_add(12) <= MAX_FRAME,
            "ADB 画面过大"
        );
        let end = &header[4..] == b"IEND";
        bytes.extend_from_slice(&header);
        let start = bytes.len();
        bytes.resize(start + size + 4, 0);
        reader.read_exact(&mut bytes[start..]).await?;
        if end {
            return Ok(bytes);
        }
    }
}
async fn capture(
    transport: &native::Transport,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    let reader = transport
        .open("exec:sh -c 'while screencap -d 0 -p 2>/dev/null; do :; done'")
        .await?;
    capture_frames(reader, shared, notifier).await
}
async fn capture_frames(
    reader: impl AsyncRead + Unpin,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    let mut reader = tokio::io::BufReader::new(reader);
    loop {
        let bytes = timeout(Duration::from_secs(15), png(&mut reader))
            .await
            .context("读取 ADB 画面超时")??;
        let frame = tokio::task::spawn_blocking(move || -> Result<Frame> {
            let (width, height) = image::ImageReader::with_format(
                std::io::Cursor::new(bytes.as_slice()),
                image::ImageFormat::Png,
            )
            .into_dimensions()?;
            ensure!(
                u64::from(width) * u64::from(height) <= 8 * 1024 * 1024,
                "ADB 画面像素超过限制"
            );
            let mut decoder = image::ImageReader::with_format(
                std::io::Cursor::new(bytes),
                image::ImageFormat::Png,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(64 * 1024 * 1024);
            decoder.limits(limits);
            let image = decoder.decode()?.into_rgb8();
            let (w, h) = image.dimensions();
            ensure!(
                w as u64 * h as u64 <= 8 * 1024 * 1024,
                "ADB 画面像素超过限制"
            );
            let pixels = image
                .pixels()
                .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
                .collect();
            Ok(Frame {
                width: w as u16,
                height: h as u16,
                pixels,
                region: None,
            })
        })
        .await??;
        if let Ok(mut state) = shared.lock() {
            state.frame = Some(frame);
            state.status = Status::Ready;
        }
        wake(notifier);
    }
}
struct Control {
    input: WriteHalf<DuplexStream>,
    output: BufReader<ReadHalf<DuplexStream>>,
}
impl Control {
    async fn open(transport: &native::Transport) -> Result<Self> {
        let (output, input) = tokio::io::split(transport.open("exec:sh").await?);
        Ok(Self {
            input,
            output: BufReader::new(output),
        })
    }
    async fn send(&mut self, quoted: &str) -> Result<()> {
        timeout(Duration::from_secs(5), async {
            self.input
                .write_all(
                    format!("input {quoted}; printf '\\n__TOMATO_ADB_ACK__:%s\\n' \"$?\"\n")
                        .as_bytes(),
                )
                .await?;
            self.input.flush().await?;
            let mut limited = (&mut self.output).take(65537);
            let mut total = 0;
            loop {
                let mut line = vec![];
                let read = limited.read_until(b'\n', &mut line).await?;
                total += read;
                ensure!(read > 0 && total <= 65536, "ADB 控制通道已断开或输出过大");
                let line = String::from_utf8_lossy(&line);
                if let Some(status) = line.trim().strip_prefix("__TOMATO_ADB_ACK__:") {
                    ensure!(status == "0", "ADB input 命令失败，退出码 {status}");
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await
        .context("ADB 控制输入超时")?
    }
}
fn input_notice(shared: &Arc<Mutex<Shared>>, notifier: &Notifier, message: String) {
    if let Ok(mut state) = shared.lock() {
        state.desktop_name = message;
    }
    wake(notifier);
}
async fn inputs(
    transport: &native::Transport,
    input: &mut pool::Inputs,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    let mut touch = std::collections::BTreeMap::new();
    let mut control = Control::open(transport).await.ok();
    let mut pending = None;
    loop {
        let event = match pending.take() {
            Some(event) => event,
            None => match input.recv().await {
                Some(event) => event,
                None => break,
            },
        };
        let owner = event.owner;
        if !input.active(&event) {
            continue;
        }
        let Some(event) = event.input else {
            touch.remove(&owner);
            continue;
        };
        let args: Vec<String> = match event {
            Input::AndroidKey(code) => vec!["keyevent".into(), code.to_string()],
            Input::ScanCode {
                code,
                extended,
                down: true,
            } => {
                let key = match (code, extended) {
                    (1, _) => 4,
                    (14, _) => 67,
                    (15, _) => 61,
                    (28, _) => 66,
                    (0x48, true) => 19,
                    (0x50, true) => 20,
                    (0x4b, true) => 21,
                    (0x4d, true) => 22,
                    (0x47, true) => 3,
                    (0x53, true) => 112,
                    _ => continue,
                };
                vec!["keyevent".into(), key.to_string()]
            }
            Input::Unicode { value, down: true } => {
                let mut text = String::new();
                let mut next = Some((value, true));
                let mut count = 0;
                while let Some((value, down)) = next.take() {
                    if down {
                        if (32..=126).contains(&value) {
                            if value == 32 {
                                text.push_str("%s");
                            } else {
                                text.push(char::from_u32(u32::from(value)).unwrap());
                            }
                        } else {
                            input_notice(shared, notifier, "ADB 键盘输入仅支持 ASCII 文本".into());
                        }
                    }
                    count += 1;
                    if count >= 512 {
                        break;
                    }
                    match input.try_recv() {
                        Ok(pool::Event {
                            owner: next_owner,
                            input: Some(Input::Unicode { value, down }),
                        }) if next_owner == owner => next = Some((value, down)),
                        Ok(other) => {
                            pending = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if text.is_empty() {
                    continue;
                }
                vec!["text".into(), text]
            }
            Input::Pointer {
                x,
                y,
                button: Some(1),
                down: true,
                ..
            } => {
                touch.insert(owner, (x, y, Instant::now()));
                continue;
            }
            Input::Pointer {
                x,
                y,
                button: Some(1),
                down: false,
                ..
            } => {
                let Some((sx, sy, time)) = touch.remove(&owner) else {
                    continue;
                };
                let elapsed = time.elapsed().as_millis().clamp(1, 10000);
                if sx.abs_diff(x) < 5 && sy.abs_diff(y) < 5 && elapsed < 350 {
                    vec!["tap".into(), x.to_string(), y.to_string()]
                } else {
                    vec![
                        "swipe".into(),
                        sx.to_string(),
                        sy.to_string(),
                        x.to_string(),
                        y.to_string(),
                        elapsed.to_string(),
                    ]
                }
            }
            Input::Pointer {
                button: Some(2),
                down: true,
                ..
            } => vec!["keyevent".into(), "4".into()],
            Input::Pointer { x, y, wheel, .. } if wheel != 0 => {
                let next = if wheel > 0 {
                    y.saturating_add(240)
                } else {
                    y.saturating_sub(240)
                };
                vec![
                    "swipe".into(),
                    x.to_string(),
                    y.to_string(),
                    x.to_string(),
                    next.to_string(),
                    "180".into(),
                ]
            }
            _ => continue,
        };
        let quoted = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let result = match &mut control {
            Some(channel) => channel.send(&quoted).await,
            None => match Control::open(transport).await {
                Ok(mut channel) => {
                    let result = channel.send(&quoted).await;
                    control = Some(channel);
                    result
                }
                Err(error) => Err(error),
            },
        };
        if let Err(error) = result {
            control = None;
            input_notice(shared, notifier, format!("ADB 输入失败：{error:#}"));
        }
    }
    Ok(())
}
async fn run(
    connection: Connection,
    input: &mut pool::Inputs,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    let endpoint = endpoint(&connection)?;
    let transport = native::Transport::connect(&endpoint).await?;
    if let Ok(mut state) = shared.lock() {
        state.desktop_name = endpoint.clone();
    }
    wake(notifier);
    if connection.text("adbDisplayMode") != "PNG" {
        match scrcpy::Session::open(&connection, &transport).await {
            Ok(stream) => return stream.run(input, shared, notifier).await,
            Err(error) => input_notice(
                shared,
                notifier,
                format!("H.264 启动失败，使用稳定截图：{error:#}"),
            ),
        }
    }
    if let Ok(mut state) = shared.lock() {
        state.adb_video = Some(VideoStats {
            mode: "PNG",
            ..Default::default()
        });
    }
    tokio::select! {
        result = capture(&transport, shared, notifier) => result,
        result = inputs(&transport, input, shared, notifier) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_adb_endpoints_keep_scheme_explicit_port_and_ipv6() {
        for (host, expected) in [
            ("adb://device.example", "device.example:5555"),
            ("[::1]", "[::1]:5555"),
            ("::1", "[::1]:5555"),
            ("[::1]:6000", "[::1]:6000"),
            ("127.0.0.1:6000", "127.0.0.1:6000"),
        ] {
            let mut c = Connection::new("ADB");
            c.set("host", host);
            assert_eq!(endpoint(&c).unwrap(), expected);
        }
    }
    #[tokio::test]
    async fn png_stream_bounds_chunks_and_accepts_consecutive_frames() -> Result<()> {
        let image = image::RgbImage::new(2, 2);
        let mut encoded = std::io::Cursor::new(Vec::new());
        image.write_to(&mut encoded, image::ImageFormat::Png)?;
        let mut bytes = encoded.get_ref().clone();
        bytes.extend_from_slice(encoded.get_ref());
        let mut stream = bytes.as_slice();
        assert_eq!(png(&mut stream).await?, *encoded.get_ref());
        assert_eq!(png(&mut stream).await?, *encoded.get_ref());
        assert!(stream.is_empty());
        let mut malformed = b"\x89PNG\r\n\x1a\n\xff\xff\xff\xffIDAT".as_slice();
        assert!(png(&mut malformed).await.is_err());
        Ok(())
    }
}
