//! Native RFB adapter: RFC 6143 and the encodings offered by the Java client.
mod framebuffer;
mod input;
use super::*;
use anyhow::{Context, bail};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::{Duration, timeout},
};

pub(super) struct Worker(pub tokio::task::JoinHandle<()>);
impl Drop for Worker {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub(super) fn validate(connection: &Connection) -> Result<()> {
    ensure!(
        !connection.text("host").trim().is_empty(),
        "VNC 主机不能为空"
    );
    ensure!(u16::try_from(vnc_port(connection)).is_ok(), "VNC 端口无效");
    Ok(())
}
pub(super) async fn run(
    mut connection: Connection,
    inputs: mpsc::Receiver<Input>,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
    clipboard: &Arc<ClipboardBridge>,
    authentication: &Arc<Authentication>,
) -> Result<()> {
    if connection.number("port") == 0 {
        connection.set("port", vnc_port(&connection));
    }
    let _tunnel = if connection.flag("useSshTunnel") {
        Some(crate::terminal::ssh::tunnel::Tunnel::open(&connection).await?)
    } else {
        None
    };
    let connection = _tunnel
        .as_ref()
        .map(|t| t.routed(&connection))
        .unwrap_or(connection);
    let socket = timeout(
        Duration::from_secs(10),
        TcpStream::connect((
            connection.text("host").trim(),
            connection.number("port") as u16,
        )),
    )
    .await
    .context("VNC 连接超时")??;
    socket.set_nodelay(true)?;
    let (read, mut write) = socket.into_split();
    let mut read = BufReader::with_capacity(64 * 1024, read);
    let (mut screen, name) = handshake(
        &mut read,
        &mut write,
        connection.text("password"),
        authentication,
        shared,
        notifier,
    )
    .await?;
    {
        let mut state = shared
            .lock()
            .map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?;
        state.status = Status::Ready;
        state.desktop_name = name;
        state.frame = Some(screen.snapshot());
    }
    wake(notifier);
    let enabled = connection
        .options
        .get("mapClipboard")
        .and_then(serde_json::Value::as_bool)
        != Some(false);
    let initial = screen.request(false);
    let (updates, requests) = tokio::sync::watch::channel(screen.request(true));
    let (finished, mut completion) = tokio::sync::oneshot::channel();
    let bridge = clipboard.clone();
    // Independent read/write actors prevent a large clipboard write from
    // blocking framebuffer reads. Both halves are owned by abort-on-drop guards.
    let _writer = Worker(tokio::spawn(async move {
        let result = write_messages(&mut write, inputs, initial, requests, &bridge, enabled).await;
        let _ = finished.send(result);
    }));
    loop {
        let message = tokio::select! {
            result = screen.read_message(&mut read) => result?,
            result = &mut completion => {
                result.context("VNC 写入任务异常结束")??;
                shared.lock().map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?.status = Status::Closed;
                wake(notifier);
                return Ok(());
            }
        };
        match message {
            framebuffer::Message::Frame(frame) => {
                let request = [
                    3,
                    1,
                    0,
                    0,
                    0,
                    0,
                    (frame.width >> 8) as u8,
                    frame.width as u8,
                    (frame.height >> 8) as u8,
                    frame.height as u8,
                ];
                shared
                    .lock()
                    .map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?
                    .frame = Some(frame);
                updates.send_replace(request);
                wake(notifier);
            }
            framebuffer::Message::Clipboard(text) if enabled => {
                shared
                    .lock()
                    .map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?
                    .clipboard = Some(text);
                wake(notifier);
            }
            framebuffer::Message::Bell => {
                shared
                    .lock()
                    .map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?
                    .bell = true;
                wake(notifier);
            }
            framebuffer::Message::Closed => {
                shared
                    .lock()
                    .map_err(|_| anyhow::anyhow!("VNC 状态锁失效"))?
                    .status = Status::Closed;
                wake(notifier);
                return Ok(());
            }
            _ => {}
        }
    }
}
async fn write_messages<W: AsyncWrite + Unpin>(
    write: &mut W,
    mut inputs: mpsc::Receiver<Input>,
    initial: [u8; 10],
    mut requests: tokio::sync::watch::Receiver<[u8; 10]>,
    clipboard: &ClipboardBridge,
    enabled: bool,
) -> Result<()> {
    write.write_all(&initial).await?;
    let mut keyboard = input::Encoder::default();
    loop {
        tokio::select! {
            event = inputs.recv() => {
                let Some(event) = event else { return Ok(()); };
                if enabled { flush_clipboard(write, clipboard).await?; }
                let bytes = keyboard.encode(event);
                timeout(Duration::from_secs(10),write.write_all(&bytes)).await.context("VNC 输入超时")??;
                if matches!(event, Input::ScanCode {code:0x2f,down:false,..})
                    && let Ok(mut state) = clipboard.outgoing.lock()
                    && state.waiting_keyup
                { state.waiting_keyup=false; state.busy=false; }
            }
            _ = clipboard.ready.notified(), if enabled => { flush_clipboard(write, clipboard).await?; }
            changed = requests.changed() => {
                if changed.is_err() { return Ok(()); }
                let request = *requests.borrow_and_update();
                timeout(Duration::from_secs(10),write.write_all(&request)).await.context("VNC 更新请求超时")??;
            }
        }
    }
}
async fn flush_clipboard<W: AsyncWrite + Unpin>(
    write: &mut W,
    clipboard: &ClipboardBridge,
) -> Result<()> {
    let text = clipboard
        .outgoing
        .lock()
        .ok()
        .and_then(|mut state| state.pending.take());
    if let Some((text, paste)) = text {
        let length = clipboard_length(&text)?;
        let mut header = [6, 0, 0, 0, 0, 0, 0, 0];
        header[4..].copy_from_slice(&length.to_be_bytes());
        write.write_all(&header).await?;
        let mut chars = latin1(&text);
        let mut buffer = [0; 64 * 1024];
        loop {
            let mut length = 0;
            for slot in &mut buffer {
                let Some(character) = chars.next() else {
                    break;
                };
                *slot = character;
                length += 1;
            }
            if length == 0 {
                break;
            }
            timeout(Duration::from_secs(10), write.write_all(&buffer[..length]))
                .await
                .context("VNC 剪贴板发送超时")??;
        }
        if let Ok(mut state) = clipboard.outgoing.lock() {
            state.waiting_keyup = paste;
            state.busy = paste;
        }
    }
    Ok(())
}
pub(super) fn clipboard_length(text: &str) -> Result<u32> {
    let length = text.chars().count();
    ensure!(
        length <= 16 * 1024 * 1024,
        "VNC 剪贴板文本超过原版 16 MiB 上限"
    );
    Ok(length as u32)
}
pub(super) fn latin1(text: &str) -> impl Iterator<Item = u8> + '_ {
    // Java ISO_8859_1 substitutes one '?' per unsupported Unicode scalar.
    text.chars().map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
}
async fn handshake<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: &mut R,
    write: &mut W,
    password: &str,
    authentication: &Authentication,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<(framebuffer::Screen, String)> {
    let mut banner = [0; 12];
    read.read_exact(&mut banner).await?;
    ensure!(
        &banner[..8] == b"RFB 003."
            && banner[8..11].iter().all(u8::is_ascii_digit)
            && banner[11] == b'\n',
        "服务器返回了无效的 RFB 版本"
    );
    let minor = u16::from(banner[8] - b'0') * 100
        + u16::from(banner[9] - b'0') * 10
        + u16::from(banner[10] - b'0');
    let minor = if minor >= 8 {
        8
    } else if minor >= 7 {
        7
    } else {
        3
    };
    write
        .write_all(format!("RFB 003.{minor:03}\n").as_bytes())
        .await?;
    let security = if minor == 3 {
        let kind = read.read_u32().await?;
        ensure!(kind <= 2, "VNC 服务器要求不支持的认证类型: {kind}");
        kind as u8
    } else {
        let count = read.read_u8().await?;
        if count == 0 {
            bail!("{}", read_text(read).await?);
        }
        let mut offered = vec![0; count as usize];
        read.read_exact(&mut offered).await?;
        let kind = if offered.contains(&2) && !password.is_empty() {
            2
        } else if offered.contains(&1) {
            1
        } else if offered.contains(&2) {
            2
        } else {
            bail!("VNC 服务器未提供支持的认证方式（仅支持 None 和 VNC Password）");
        };
        write.write_u8(kind).await?;
        kind
    };
    if security == 0 {
        bail!("{}", read_text(read).await?);
    }
    if security == 2 {
        let prompted;
        let password = if password.is_empty() {
            shared
                .lock()
                .map_err(|_| anyhow::anyhow!("会话状态锁失效"))?
                .status = Status::PasswordRequired;
            wake(notifier);
            prompted = loop {
                let reply = authentication
                    .reply
                    .lock()
                    .map_err(|_| anyhow::anyhow!("认证状态锁失效"))?
                    .take();
                match reply {
                    Some(PasswordReply::Provided(password)) => break password,
                    Some(PasswordReply::Cancelled) => bail!("VNC 服务器需要密码"),
                    None => authentication.ready.notified().await,
                }
            };
            prompted.as_str()
        } else {
            password
        };
        let mut challenge = [0; 16];
        read.read_exact(&mut challenge).await?;
        write
            .write_all(&encrypt_challenge(challenge, password))
            .await?;
    }
    // RFB 3.3/3.7 None transitions directly to ClientInit; 3.8 has a result.
    if security == 2 || minor >= 8 {
        let result = read.read_u32().await?;
        if result != 0 {
            let reason = if minor >= 8 {
                read_text(read).await?
            } else {
                "认证被服务器拒绝".into()
            };
            bail!("{reason}");
        }
    }
    write.write_u8(1).await?; // Shared desktop.
    let width = read.read_u16().await?;
    let height = read.read_u16().await?;
    let screen = framebuffer::Screen::new(width, height)?;
    let mut format = [0; 16];
    read.read_exact(&mut format).await?;
    let name = read_text(read).await?;
    write
        .write_all(&[
            0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0,
        ])
        .await?;
    let mut encodings = vec![2, 0, 0, 3];
    for value in [5i32, 0, -223] {
        encodings.extend_from_slice(&value.to_be_bytes());
    }
    write.write_all(&encodings).await?;
    Ok((screen, name))
}
pub(super) async fn read_text<R: AsyncRead + Unpin>(read: &mut R) -> Result<String> {
    let length = read.read_u32().await? as usize;
    ensure!(length <= 16 * 1024 * 1024, "VNC 文本过长");
    let mut bytes = vec![0; length];
    read.read_exact(&mut bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
pub(super) fn encrypt_challenge(mut challenge: [u8; 16], password: &str) -> [u8; 16] {
    use des::cipher::{BlockCipherEncrypt, KeyInit};
    let mut key = [0; 8];
    for (dst, src) in key.iter_mut().zip(latin1(password)) {
        *dst = src.reverse_bits();
    }
    let cipher = des::Des::new((&key).into());
    for block in challenge.as_chunks_mut::<8>().0 {
        let mut data = des::cipher::Block::<des::Des>::default();
        data.copy_from_slice(block);
        cipher.encrypt_block(&mut data);
        block.copy_from_slice(&data);
    }
    challenge
}

#[cfg(test)]
mod tests;
