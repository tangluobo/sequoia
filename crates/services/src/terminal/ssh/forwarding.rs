//! Owned SSH L/R tunnels. Closing a mapping aborts all streams and releases its listener.
use super::*;
use serde::{Deserialize, Serialize};
mod supervisor;
pub use supervisor::{Forward, ForwardState};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mapping {
    #[serde(deserialize_with = "nullable_id")]
    pub id: String,
    #[serde(deserialize_with = "nullable_text")]
    pub name: String,
    #[serde(deserialize_with = "nullable_text")]
    pub ssh_host_id: String,
    #[serde(deserialize_with = "nullable_direction")]
    pub direction: String,
    #[serde(deserialize_with = "nullable_host")]
    pub local_host: String,
    pub local_port: u16,
    #[serde(deserialize_with = "nullable_host")]
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}
impl Default for Mapping {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            ssh_host_id: String::new(),
            direction: "L".into(),
            local_host: "127.0.0.1".into(),
            remote_host: "127.0.0.1".into(),
            local_port: 0,
            remote_port: 0,
            extra: Default::default(),
        }
    }
}
impl Mapping {
    pub fn validate(&self) -> Result<()> {
        ensure!(matches!(self.direction.as_str(), "L" | "R"), "无效映射方向");
        ensure!(!self.ssh_host_id.is_empty(), "请选择 SSH 主机");
        ensure!(
            self.local_port > 0 && self.remote_port > 0,
            "端口必须为 1–65535"
        );
        ensure!(
            !self.local_host.trim().is_empty() && !self.remote_host.trim().is_empty(),
            "地址不能为空"
        );
        Ok(())
    }
}
struct Running {
    task: tokio::task::JoinHandle<Result<()>>,
    _socket: SocketGuard,
    _session: Arc<client::Handle<Client>>,
}
impl Running {
    async fn open(mapping: Mapping, host: Connection, known_hosts: PathBuf) -> Result<Self> {
        let _permit = supervisor::permit().await?;
        mapping.validate()?;
        ensure!(
            matches!(host.kind(), "SSH" | "SFTP") && host.id == mapping.ssh_host_id,
            "引用的 SSH 主机不存在"
        );
        let listener = if mapping.direction == "L" {
            Some(
                tokio::net::TcpListener::bind((mapping.local_host.as_str(), mapping.local_port))
                    .await?,
            )
        } else {
            None
        };
        let (answers, mut input) = mpsc::channel(1);
        drop(answers);
        let events = Events {
            state: Arc::new(Mutex::new(Status::Connecting)),
            notifier: Arc::new(Mutex::new(None)),
        };
        let (sender, mut forwards) = mpsc::channel(16);
        let connected =
            connect::authenticated_forward(&host, known_hosts, &mut input, &events, Some(sender))
                .await?;
        let session = connected.session;
        if mapping.direction == "R" {
            session
                .tcpip_forward(mapping.remote_host.clone(), u32::from(mapping.remote_port))
                .await
                .context("SSH 服务器拒绝远程端口转发")?;
        }
        let session = Arc::new(session);
        let active = session.clone();
        let task: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
            let _parents = connected._parents;
            let mut jobs = tokio::task::JoinSet::new();
            let mut health = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _=health.tick()=>{ensure!(!active.is_closed(), "SSH 连接已断开");},
                    accepted=async {listener.as_ref().unwrap().accept().await},if listener.is_some() && jobs.len()<32=>{
                        let (mut stream,origin)=accepted.context("本地监听异常")?;
                        let session=active.clone();let host=mapping.remote_host.clone();let port=mapping.remote_port;
                        jobs.spawn(async move {if let Ok(Ok(channel))=tokio::time::timeout(Duration::from_secs(15),session.channel_open_direct_tcpip(host,u32::from(port),origin.ip().to_string(),u32::from(origin.port()))).await {let _=stream.set_nodelay(true);let mut remote=channel.into_stream();let _=tokio::io::copy_bidirectional(&mut stream,&mut remote).await;}});
                    },
                    channel=forwards.recv(),if listener.is_none() && jobs.len()<32=>{
                        let channel=channel.context("远程转发通道已关闭")?;let host=mapping.local_host.clone();let port=mapping.local_port;
                        jobs.spawn(async move {let mut remote=channel.into_stream();if let Ok(Ok(mut local))=tokio::time::timeout(Duration::from_secs(15),tokio::net::TcpStream::connect((host.as_str(),port))).await {let _=local.set_nodelay(true);let _=tokio::io::copy_bidirectional(&mut local,&mut remote).await;}});
                    },
                    _=jobs.join_next(),if !jobs.is_empty()=>{},
                }
            }
        });
        Ok(Self {
            task,
            _socket: connected._socket_guard,
            _session: session,
        })
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Java stores these separately from connections, under the historical `.tomata` spelling.
pub fn legacy_path() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".tomata/port-mappings.json")
}
pub struct Repository {
    pub path: PathBuf,
    original: Option<Vec<u8>>,
}
impl Repository {
    pub fn load(path: PathBuf) -> Result<(Self, Vec<Mapping>)> {
        let original = read_config(&path)?;
        let mappings: Vec<Mapping> = original
            .as_ref()
            .map(|bytes| serde_json::from_slice::<Option<Vec<Mapping>>>(bytes))
            .transpose()?
            .flatten()
            .unwrap_or_default();
        ensure!(mappings.len() <= 10000, "端口映射条目过多");
        let mut ids = std::collections::HashSet::new();
        ensure!(
            mappings
                .iter()
                .all(|m| !m.id.is_empty() && ids.insert(&m.id)),
            "端口映射 ID 为空或重复"
        );
        Ok((Self { path, original }, mappings))
    }
    pub fn save(&mut self, mappings: &[Mapping]) -> Result<()> {
        let parent = self.path.parent().context("无效映射配置路径")?;
        std::fs::create_dir_all(parent)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path.with_extension("json.rust-lock"))?;
        lock.try_lock().context("其他窗口正在保存端口映射")?;
        let current = read_config(&self.path)?;
        ensure!(
            current == self.original,
            "端口映射配置已被其他程序修改，请重新打开工具"
        );
        if let Some(bytes) = &self.original {
            let backup = self.path.with_extension("json.rust-backup");
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(backup)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    f.write_all(bytes)?;
                    f.sync_all()?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        let bytes = serde_json::to_vec_pretty(mappings)?;
        ensure!(
            bytes.len() <= 4 * 1024 * 1024 && mappings.len() <= 10000,
            "端口映射配置过大"
        );
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(&self.path)?;
        self.original = Some(bytes);
        Ok(())
    }
}
fn read_config(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 4 * 1024 * 1024, "端口映射配置过大");
            Ok(Some(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn nullable_text<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}
fn nullable_id<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()))
}
fn nullable_host<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_else(|| "127.0.0.1".into()))
}
fn nullable_direction<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_else(|| "L".into()))
}
