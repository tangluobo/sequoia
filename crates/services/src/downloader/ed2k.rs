//! Native eD2k adapter; the queue and legacy repository remain protocol independent.
use super::*;
use std::{
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicU32,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tomato_ed2k::{
    kad::{KadConfig, KadService},
    types::Ed2kFileLink,
};

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize, PartialEq)]
struct Owner {
    version: u32,
    task: String,
    hash: String,
    size: u64,
}

struct Target {
    path: PathBuf,
    owner_path: PathBuf,
    _lease: std::fs::File,
}

fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    value.into()
}

fn reserve(record: &Record, link: &Ed2kFileLink) -> Result<Target> {
    let directory = PathBuf::from(&record.directory);
    std::fs::create_dir_all(&directory)?;
    let identity = Owner {
        version: 1,
        task: record.id.clone(),
        hash: link.file_hash.clone(),
        size: link.file_size,
    };
    let name = sanitize_name(&record.file_name);
    let path = Path::new(&name);
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("download");
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| format!(".{v}"))
        .unwrap_or_default();
    for index in 0..10000 {
        let name = if index == 0 {
            name.clone()
        } else {
            format!("{stem} ({index}){extension}")
        };
        let path = directory.join(name);
        if path.exists() {
            continue;
        }
        let owner_path = suffix(&path, ".tomato-ed2k.owner");
        let can_resume = record.target_file.as_deref().map(Path::new) == Some(path.as_path());
        let lease = if owner_path.exists() {
            if !can_resume {
                continue;
            }
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&owner_path)?;
            file.try_lock()
                .context("该 eD2k 分片正由另一个下载任务使用")?;
            ensure!(file.metadata()?.len() <= 4096, "eD2k 分片归属记录过大");
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let saved = serde_json::from_slice::<Owner>(&bytes)
                .context("eD2k 分片归属记录损坏，已保留原文件")?;
            // Java rewrites the shared history without rustId. Resume is bound
            // to the explicitly selected path and content identity, while the
            // exclusive lease still prevents two Rust tasks writing it together.
            ensure!(
                saved.version == identity.version
                    && saved.hash.eq_ignore_ascii_case(&identity.hash)
                    && saved.size == identity.size,
                "eD2k 分片不属于此任务，已保留原文件"
            );
            file
        } else {
            if tomato_ed2k::partial::part_path(&path).exists()
                || tomato_ed2k::partial::met_path(&path).exists()
            {
                continue;
            }
            let mut file = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&owner_path)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            };
            file.try_lock()?;
            file.write_all(&serde_json::to_vec(&identity)?)?;
            file.sync_all()?;
            file
        };
        return Ok(Target {
            path,
            owner_path,
            _lease: lease,
        });
    }
    anyhow::bail!("无法生成可用的 eD2k 下载文件名")
}

fn identity(profile: &Path) -> Result<[u8; 16]> {
    std::fs::create_dir_all(profile)?;
    let path = profile.join("user-hash");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            let hash = tomato_ed2k::protocol::new_user_hash();
            file.write_all(&hash)?;
            file.sync_all()?;
            Ok(hash)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut hash = [0; 16];
            let mut file = std::fs::File::open(path)?;
            ensure!(
                file.metadata()?.len() == 16,
                "eD2k 客户端标识损坏，已保留原文件"
            );
            file.read_exact(&mut hash)?;
            Ok(hash)
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Network {
    servers: Vec<String>,
    kad: bool,
    loopback_only: bool,
}
impl Default for Network {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            kad: true,
            loopback_only: false,
        }
    }
}
impl Network {
    fn load(profile: &Path) -> Result<Self> {
        let path = profile.join("rust-ed2k/network.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        ensure!(
            std::fs::metadata(&path)?.len() <= 32768,
            "eD2k 网络配置过大"
        );
        let network: Self =
            serde_json::from_slice(&std::fs::read(path)?).context("eD2k 网络配置无效")?;
        ensure!(network.servers.len() <= 128, "eD2k 服务器数量超过 128");
        for server in &network.servers {
            let addr: std::net::SocketAddrV4 =
                server.parse().context("eD2k 服务器必须为 IPv4:端口")?;
            ensure!(
                addr.port() != 0 && (!network.loopback_only || addr.ip().is_loopback()),
                "隔离 eD2k 配置只允许回环服务器"
            );
        }
        ensure!(
            !network.loopback_only || (!network.kad && !network.servers.is_empty()),
            "隔离 eD2k 配置必须禁用公网 Kad 并指定回环服务器"
        );
        Ok(network)
    }
}

fn servers(profile: &Path) -> Vec<String> {
    let mut servers = Vec::new();
    for path in [
        profile.join("rust-ed2k/server.met"),
        profile.join("server.met"),
        profile.join("server2.met"),
    ] {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 1024 * 1024)
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(entries) = tomato_ed2k::server_list::parse_met(&bytes)
        {
            servers.extend(entries);
        }
    }
    // Bundled upstream bootstrap addresses also work before a cache exists.
    servers.extend(
        tomato_ed2k::server_list::server_list(&[])
            .into_iter()
            .map(|s| format!("{}:{}", s.ip, s.port)),
    );
    let mut seen = std::collections::HashSet::new();
    servers.retain(|s| seen.insert(s.clone()));
    servers.truncate(128);
    servers
}

async fn refresh_list(client: &reqwest::Client, profile: &Path, file: &'static str) {
    // Same bootstrap source as Java. Keep its caches untouched; bound both time
    // and bytes, and fall back to the cache/bundled seeds when unavailable.
    let _ = tokio::time::timeout(Duration::from_secs(6), async {
        let mut response = client
            .get(format!("https://upd.emule-security.org/{file}"))
            .send()
            .await?
            .error_for_status()?;
        if response.content_length().is_some_and(|n| n > 1024 * 1024) {
            return Ok::<(), anyhow::Error>(());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Ok(());
            }
            bytes.extend_from_slice(&chunk);
        }
        let valid = match file {
            "server.met" => {
                tomato_ed2k::server_list::parse_met(&bytes).is_ok_and(|s| !s.is_empty())
            }
            "nodes.dat" => tomato_ed2k::kad::nodes::parse(&bytes).is_ok_and(|s| !s.is_empty()),
            _ => false,
        };
        if valid {
            tomato_core::config::atomic_write(&profile.join("rust-ed2k").join(file), &bytes)?;
        }
        Ok(())
    })
    .await;
}

fn kad_seeds(profile: &Path) -> Vec<std::net::SocketAddrV4> {
    let mut seeds = Vec::new();
    for path in [
        profile.join("rust-ed2k/nodes.dat"),
        profile.join("nodes.dat"),
    ] {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 1024 * 1024)
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(entries) = tomato_ed2k::kad::nodes::parse(&bytes)
        {
            seeds.extend(entries);
        }
    }
    seeds.truncate(32);
    seeds
}

async fn refresh_servers(client: &reqwest::Client, profile: &Path, kad: bool) {
    if kad {
        tokio::join!(
            refresh_list(client, profile, "server.met"),
            refresh_list(client, profile, "nodes.dat")
        );
    } else {
        refresh_list(client, profile, "server.met").await;
    }
}

pub(super) async fn run(
    link: Ed2kFileLink,
    profile: &Path,
    row: &Arc<Mutex<Record>>,
    control: &crate::backup::Control,
    revision: &Arc<AtomicU64>,
    wake: &Wake,
    client: &reqwest::Client,
) -> Result<PathBuf> {
    let cache = profile.join("rust-ed2k");
    let network = Network::load(profile)?;
    let hash = identity(&cache)?;
    let source_servers = if network.servers.is_empty() {
        tokio::select! {
            _ = control.wait_cancelled() => return Err(anyhow::anyhow!("已取消")),
            _ = refresh_servers(client, profile, network.kad) => {},
        }
        servers(profile)
    } else {
        network.servers
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0)).await?;
    let port = listener.local_addr()?.port();
    let cancel = CancellationToken::new();
    let incoming = listen(listener, hash, port, cancel.clone());
    let kad = if network.kad {
        tokio::select! {
            _ = control.wait_cancelled() => { cancel.cancel(); return Err(anyhow::anyhow!("已取消")); },
        service = KadService::bind(KadConfig::new(cache, 0, port).with_bootstrap_seeds(kad_seeds(profile))) => service.ok(),
        }
    } else {
        None
    };
    let options = Options {
        servers: source_servers,
        connector: tomato_ed2k::transport::Connector {
            max_connections: row.lock().unwrap().requested_threads.clamp(1, 32),
            client_hash: Some(hash),
            loopback_only: network.loopback_only,
        },
        port,
        kad: kad.clone(),
    };
    let result = transfer(link, row, control, revision, wake, options).await;
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), incoming).await;
    if let Some(kad) = kad {
        let _ = tokio::time::timeout(Duration::from_secs(2), kad.shutdown()).await;
    }
    result
}

struct Options {
    servers: Vec<String>,
    connector: tomato_ed2k::transport::Connector,
    port: u16,
    kad: Option<Arc<KadService>>,
}

async fn transfer(
    mut link: Ed2kFileLink,
    row: &Arc<Mutex<Record>>,
    control: &crate::backup::Control,
    revision: &Arc<AtomicU64>,
    wake: &Wake,
    options: Options,
) -> Result<PathBuf> {
    let record = row.lock().unwrap().clone();
    let target = reserve(&record, &link)?;
    link.file_name = target
        .path
        .file_name()
        .context("eD2k 目标文件名无效")?
        .to_string_lossy()
        .into_owned();
    let directory = target
        .path
        .parent()
        .context("eD2k 保存目录无效")?
        .to_string_lossy()
        .into_owned();
    {
        let mut record = row.lock().unwrap();
        record.protocol = "ED2K".into();
        record.file_name = link.file_name.clone();
        record.target_file = Some(target.path.display().to_string());
        record.total_bytes = link.file_size as i64;
        record.status = "ED2K · 正在查找文件来源".into();
    }
    changed(revision, wake);
    let total = Arc::new(AtomicU64::new(link.file_size));
    let completed = Arc::new(AtomicU64::new(0));
    let speed = Arc::new(AtomicU64::new(0));
    let connections = Arc::new(AtomicU32::new(0));
    let status = Arc::new(parking_lot::Mutex::new(None));
    let cancel = CancellationToken::new();
    let mut tick = tokio::time::interval(Duration::from_millis(300));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let result = loop {
        let job = tomato_ed2k::run_ed2k_download_with_proxy(
            &link,
            &directory,
            options.servers.clone(),
            options.port,
            options.kad.as_ref().and_then(|k| k.advertised_udp_port()),
            options.kad.clone(),
            status.clone(),
            total.clone(),
            completed.clone(),
            speed.clone(),
            connections.clone(),
            cancel.clone(),
            options.connector.clone(),
        );
        tokio::pin!(job);
        let outcome = loop {
            tokio::select! {
                _ = control.wait_cancelled() => {
                    cancel.cancel();
                    let _ = tokio::time::timeout(Duration::from_secs(2), &mut job).await;
                    break Err("已取消".to_owned());
                }
                result = &mut job => break result,
                _ = tick.tick() => {
                    let mut record = row.lock().unwrap();
                    record.downloaded_bytes = completed.load(Ordering::Relaxed);
                    record.actual_threads = connections.load(Ordering::Relaxed) as usize;
                    record.status = if speed.load(Ordering::Relaxed) > 0 { format!("ED2K · {} 连接 · 下载并校验分片", record.actual_threads) }
                        else { format!("ED2K · {} 连接 · 查找来源 / 等待上传队列", record.actual_threads) };
                    drop(record);
                    changed(revision, wake);
                }
            }
        };
        match outcome {
            Ok(path) => break Ok(path),
            Err(_) if control.cancelled() => break Err(anyhow::anyhow!("已取消")),
            Err(error)
                if [
                    "Failed to connect to ",
                    "No servers available",
                    "No usable sources from server",
                    "Server disconnected:",
                    "Server event channel closed",
                    "Send channel closed",
                    "Not connected",
                ]
                .iter()
                .any(|prefix| error.starts_with(prefix)) =>
            {
                row.lock().unwrap().status =
                    "ED2K · 暂无可用来源，15 秒后继续查找（可取消）".into();
                changed(revision, wake);
                tokio::select! { _ = control.wait_cancelled() => break Err(anyhow::anyhow!("已取消")), _ = tokio::time::sleep(Duration::from_secs(15)) => {} }
            }
            Err(error) => break Err(anyhow::anyhow!("eD2k 下载失败：{error}")),
        }
    };
    row.lock().unwrap().actual_threads = 0;
    if result.is_ok() {
        row.lock().unwrap().downloaded_bytes = link.file_size;
        let owner_path = target.owner_path.clone();
        drop(target);
        std::fs::remove_file(owner_path)?;
    }
    result
}

fn listen(
    listener: tokio::net::TcpListener,
    hash: [u8; 16],
    port: u16,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = jobs.join_next(), if !jobs.is_empty() => {},
                incoming = listener.accept(), if jobs.len() < 8 => {
                    let Ok((mut stream, _)) = incoming else { break };
                    jobs.spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(5), async {
                            let mut header = [0; 6]; stream.read_exact(&mut header).await?;
                            let length = u32::from_le_bytes(header[1..5].try_into().unwrap());
                            if header[0] == 0xe3 && header[5] == 1 && (23..=4096).contains(&length) {
                                let mut bytes = vec![0; length as usize - 1]; stream.read_exact(&mut bytes).await?;
                                let mut hello = tomato_ed2k::protocol::build_hello_client(&hash, 0, port, 0, 0);
                                hello.opcode = 0x4c; hello.payload.remove(0);
                                stream.write_all(&hello.encode()).await?;
                            }
                            Ok::<_, std::io::Error>(())
                        }).await;
                    });
                }
            }
        }
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
    })
}
