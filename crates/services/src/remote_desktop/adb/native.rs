//! Direct device transport. Each stream owns one service channel on the authenticated socket.
use super::*;
use libadb::{
    AdbKey, ConnectionConfig, HostBanner, Splittable, TlsClientConfig, TlsIdentity, TokioTcp,
    Writer,
    channel::{ChannelId, SelectResult},
    keys::{KeyName, store},
    transport::tls::MaybeTls,
};
use rsa::rand_core::OsRng;
use std::io::{ErrorKind, Write};
use tokio::{io::DuplexStream, net::TcpStream};
use zeroize::Zeroize;
#[cfg(test)]
mod tests;

type Socket = MaybeTls<TokioTcp>;
pub(super) type ChannelError = libadb::Error<libadb::transport::tls::TlsError<std::io::Error>>;
type Sender = Writer<<Socket as Splittable>::WriteHalf>;
const PACKET: usize = 64 * 1024;
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct AdbPairing {
    address: String,
    #[serde(skip_serializing)]
    code: String,
}
impl std::fmt::Debug for AdbPairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbPairing")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}
impl Drop for AdbPairing {
    fn drop(&mut self) {
        self.code.zeroize();
    }
}
impl AdbPairing {
    pub fn new(address: String, code: String) -> Self {
        Self { address, code }
    }
    fn validate(&self) -> Result<&str> {
        let address = self.address.trim();
        let (host, port) = address.rsplit_once(':').context("请填写配对地址和端口")?;
        ensure!(
            !host.is_empty()
                && (!host.contains(':') || (host.starts_with('[') && host.ends_with(']')))
                && port.parse::<u16>().is_ok_and(|port| port != 0),
            "配对地址格式无效，请输入配对页面显示的 IP 和端口"
        );
        ensure!(
            self.code.len() == 6 && self.code.bytes().all(|byte| byte.is_ascii_digit()),
            "请输入 Android 设备显示的六位配对码"
        );
        Ok(address)
    }
    pub(crate) async fn pair(&self) -> Result<()> {
        let address = self.validate()?;
        timeout(Duration::from_secs(20), async {
            let socket = TcpStream::connect(address)
                .await
                .context("无法连接无线配对端口")?;
            socket.set_nodelay(true)?;
            let key = tokio::task::spawn_blocking(device_key).await??;
            let mut transport = MaybeTls::plain(TokioTcp::new(socket));
            match libadb::pairing::pair(&mut transport, &tls(&key)?, &key, &self.code, &mut OsRng)
                .await
            {
                Ok(_) => Ok(()),
                Err(libadb::pairing::PairingError::WrongCode) => {
                    anyhow::bail!("配对码不匹配，请在设备上重新打开配对页面")
                }
                Err(error) => {
                    Err(anyhow::Error::new(error).context("无线配对失败，请核对地址和配对码"))
                }
            }
        })
        .await
        .context("无线配对超时，请重新打开设备的配对页面")?
    }
}

fn tls(key: &AdbKey) -> Result<TlsClientConfig> {
    Ok(TlsClientConfig::adb(&TlsIdentity::from_key(
        key, &mut OsRng,
    )?)?)
}

#[derive(Clone)]
pub(super) struct Transport(Arc<Connected>);

struct Connected {
    writer: Sender,
    _reader: vnc::Worker,
}

impl Transport {
    pub(super) async fn connect(endpoint: &str) -> Result<Self> {
        let socket = timeout(OPEN_TIMEOUT, TcpStream::connect(endpoint))
            .await
            .context("连接 ADB 设备超时")?
            .with_context(|| format!("无法直连 ADB 设备 {endpoint}"))?;
        let key = tokio::task::spawn_blocking(device_key).await??;
        Self::authenticate(socket, key).await
    }

    async fn authenticate(socket: TcpStream, key: AdbKey) -> Result<Self> {
        socket.set_nodelay(true)?;
        let tls = tls(&key)?;
        let config = ConnectionConfig::new()
            .with_max_payload(PACKET as u32)
            .with_initial_ack_bytes(2 * PACKET as u32)
            .with_max_rx_per_channel(4 * PACKET)
            .with_max_channels(16);
        let socket = MaybeTls::plain(TokioTcp::new(socket));
        let connection = timeout(
            Duration::from_secs(30),
            libadb::Connection::connect_tls(socket, key, HostBanner::Features(&[]), config, &tls),
        )
        .await
        .context("ADB 认证超时，请在 Android 设备上确认调试授权")?
        .context("ADB 握手失败，请检查连接端口和设备授权，首次连接可在编辑连接中无线配对")?;
        let (reader, writer) = connection.split()?;
        let reader = vnc::Worker(tokio::spawn(async move {
            let _ = reader.run().await;
        }));
        Ok(Self(Arc::new(Connected {
            writer,
            _reader: reader,
        })))
    }

    async fn channel(&self, service: &str) -> Result<ChannelId, ChannelError> {
        // ADB service names are NUL terminated on the wire.
        self.0
            .writer
            .open_channel(format!("{service}\0").as_bytes())
            .await
    }

    pub(super) async fn open(&self, service: &str) -> Result<DuplexStream> {
        let channel = timeout(OPEN_TIMEOUT, self.channel(service))
            .await
            .context("打开 ADB 通道超时")??;
        let (stream, peer) = tokio::io::duplex(PACKET);
        let writer = self.0.writer.clone();
        tokio::spawn(async move {
            let _ = relay(&writer, channel, peer).await;
            let _ = timeout(OPEN_TIMEOUT, writer.close_channel(channel)).await;
        });
        Ok(stream)
    }

    pub(super) async fn push(&self, remote: &str, bytes: &[u8]) -> Result<()> {
        let mut writer = self.0.writer.clone();
        let mut buffer = vec![0; PACKET + 8];
        let mut sync =
            timeout(OPEN_TIMEOUT, libadb::sync::open(&mut writer, &mut buffer)).await??;
        let sent = timeout(
            Duration::from_secs(15),
            sync.send(remote, libadb::sync::FileMode(0o100644), 0, bytes),
        )
        .await;
        let closed = timeout(OPEN_TIMEOUT, sync.quit()).await;
        sent.context("上传 scrcpy 服务超时")??;
        closed??;
        Ok(())
    }
}

fn device_key() -> Result<AdbKey> {
    let name = KeyName::new("Tomato@host")?;
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    if std::env::var_os("TOMATO_DATA_DIR").is_none()
        && let Some(directory) = std::env::var_os(home).map(|p| PathBuf::from(p).join(".android"))
        && directory.join("adbkey").try_exists()?
    {
        return Ok(store::load(&directory, &mut OsRng, name)?);
    }
    let directory = tomato_core::Store::default_path().with_file_name("adb");
    load_or_create_key(&directory, name).context("加载或生成 Tomato ADB 调试密钥失败")
}

fn load_or_create_key(directory: &Path, name: KeyName<'_>) -> Result<AdbKey> {
    let path = directory.join("adbkey");
    if path.try_exists()? {
        return Ok(store::load(directory, &mut OsRng, name)?);
    }
    std::fs::create_dir_all(directory)?;
    let key = AdbKey::generate(&mut OsRng, name)?;
    // The temp file is private and a concurrent process can only publish a complete key.
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(key.to_pkcs8_pem()?.as_bytes())?;
    file.as_file().sync_all()?;
    match file.persist_noclobber(path) {
        Ok(_) => Ok(key),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
            Ok(store::load(directory, &mut OsRng, name)?)
        }
        Err(error) => Err(error.error.into()),
    }
}

async fn relay(writer: &Sender, channel: ChannelId, peer: DuplexStream) -> Result<()> {
    let (mut local_read, mut local_write) = tokio::io::split(peer);
    let mut incoming = vec![0; PACKET];
    let mut outgoing = [0; 4096];
    loop {
        match writer
            .select_channel(channel, &mut incoming, local_read.read(&mut outgoing))
            .await?
        {
            SelectResult::Data(size) => local_write.write_all(&incoming[..size]).await?,
            SelectResult::Interrupted(Ok(0)) => return Ok(()),
            SelectResult::Interrupted(size) => {
                timeout(
                    OPEN_TIMEOUT,
                    writer.write_channel(channel, &outgoing[..size?]),
                )
                .await??;
            }
        }
    }
}
