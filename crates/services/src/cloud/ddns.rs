//! One cancellable scheduler for legacy ddns.json. No per-record timers or credential copies in UI.
use super::*;
use anyhow::Context;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    #[serde(default, deserialize_with = "legacy_text")]
    pub connection_id: String,
    #[serde(default, deserialize_with = "legacy_text")]
    pub domain_name: String,
    #[serde(default, deserialize_with = "legacy_text")]
    pub record_id: String,
    #[serde(default, deserialize_with = "legacy_text")]
    pub rr: String,
    #[serde(rename = "type")]
    #[serde(default, deserialize_with = "legacy_text")]
    pub kind: String,
    #[serde(default, deserialize_with = "legacy_text")]
    pub ttl: String,
    #[serde(default, deserialize_with = "legacy_text")]
    pub line: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
pub fn path() -> PathBuf {
    tomato_core::Store::default_path().with_file_name("ddns.json")
}
fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::File::open(path) {
        Ok(f) => {
            let mut b = Vec::new();
            f.take(1024 * 1024 + 1).read_to_end(&mut b)?;
            ensure!(b.len() <= 1024 * 1024, "DDNS 配置超过 1 MiB");
            Ok(Some(b))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let bytes = read(path)?;
    let list = match bytes {
        None => Vec::new(),
        Some(b) => serde_json::from_slice::<Option<Vec<Entry>>>(&b)?.unwrap_or_default(),
    };
    ensure!(list.len() <= 10000, "DDNS 配置条目过多");
    Ok(list)
}
pub fn toggle(path: &Path, c: &Connection, domain: &str, r: &Record, enabled: bool) -> Result<()> {
    ensure!(!r.record_id.is_empty(), "请先保存 DNS 记录");
    ensure!(!enabled || r.kind == "A", "DDNS 仅支持 A 记录");
    let parent = path.parent().context("DDNS 配置路径无效")?;
    std::fs::create_dir_all(parent)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("json.rust-lock"))?;
    lock.try_lock().context("其他窗口正在保存 DDNS 配置")?;
    let before = read(path)?;
    let mut entries = load(path)?;
    let previous = entries
        .iter()
        .find(|e| e.connection_id == c.id && e.record_id == r.record_id)
        .cloned();
    entries.retain(|e| !(e.connection_id == c.id && e.record_id == r.record_id));
    if enabled {
        entries.push(Entry {
            connection_id: c.id.clone(),
            domain_name: domain.into(),
            record_id: r.record_id.clone(),
            rr: r.rr.clone(),
            kind: r.kind.clone(),
            ttl: r.ttl.to_string(),
            line: r.line.clone(),
            extra: previous.map(|e| e.extra).unwrap_or_default(),
        });
    }
    ensure!(read(path)? == before, "DDNS 配置已被其他程序修改，请重试");
    if let Some(b) = before {
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path.with_extension("json.before-rust.bak"))
        {
            Ok(mut f) => {
                f.write_all(&b)?;
                f.sync_all()?
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let b = serde_json::to_vec_pretty(&entries)?;
    ensure!(b.len() <= 1024 * 1024, "DDNS 配置过大");
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.write_all(&b)?;
    f.as_file().sync_all()?;
    f.persist(path)?;
    WAKE.notify_waiters();
    Ok(())
}
pub struct Runner {
    task: tokio::task::JoinHandle<()>,
    notice: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}
impl Runner {
    pub fn start(config: PathBuf, connections: PathBuf, runtime: &tokio::runtime::Handle) -> Self {
        Self::with_sources(
            config,
            connections,
            runtime,
            Duration::from_secs(60),
            sources(),
        )
    }
    pub fn with_sources(
        config: PathBuf,
        connections: PathBuf,
        runtime: &tokio::runtime::Handle,
        interval: Duration,
        sources: Vec<String>,
    ) -> Self {
        let notice = std::sync::Arc::new(std::sync::Mutex::new(None));
        let notify = notice.clone();
        let task=runtime.spawn(async move{
            let client=match detector_client(){Ok(c)=>c,Err(_)=>return};
            let mut successful=BTreeMap::<(String,String),String>::new();
            loop{
                let change=WAKE.notified();tokio::pin!(change);change.as_mut().enable();
                let p=config.clone();let c=connections.clone();
                let loaded=tokio::task::spawn_blocking(move||->Result<_>{let entries=load(&p)?;let store=if entries.is_empty(){tomato_core::Store::default()}else{tomato_core::Store::load(&c)?};Ok((entries,store))}).await;
                let result=async{
                    let (entries,store)=loaded??;
                    if entries.is_empty(){successful.clear();return Ok::<_,anyhow::Error>(())}
                    let ip=public_ip_from(&client,&sources).await?;
                    successful.retain(|key,_|entries.iter().any(|e|e.connection_id==key.0&&e.record_id==key.1));
                    for e in entries{
                        let key=(e.connection_id.clone(),e.record_id.clone());if successful.get(&key)==Some(&ip){continue}
                        let Some(c)=store.connections.iter().find(|c|c.id==e.connection_id)else{continue};
                        let r=Record{record_id:e.record_id,rr:e.rr,kind:e.kind,value:ip.clone(),ttl:e.ttl.parse().unwrap_or(600),line:e.line,..Default::default()};
                        let result=match super::api::Client::new(c){Ok(c)=>c.save_record(&e.domain_name,&r).await.map(|_|()),Err(e)=>Err(e)};
                        if result.is_ok() || result.as_ref().is_err_and(|e|e.to_string().contains("DomainRecordDuplicate")){successful.insert(key,ip.clone());}
                        else if let Err(error)=result && let Ok(mut notice)=notify.lock(){*notice=Some(format!("DDNS 更新 {}.{} 失败：{}",r.rr,e.domain_name,error));}
                    }Ok(())
                }.await;
                if let Err(error)=result && let Ok(mut notice)=notify.lock(){*notice=Some(format!("DDNS 检测失败：{error}"));}
                tokio::select!{_=tokio::time::sleep(interval.max(Duration::from_millis(20)))=>{},_=change=>{}}
            }
        });
        Self { task, notice }
    }
    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().ok()?.take()
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.task.abort();
    }
}
// Keep detection independent from API clients: a proxy reports its own egress address.
fn detector_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("curl/8.0")
        .build()?)
}
fn sources() -> Vec<String> {
    [
        "https://gitlab.tangluobo.com/ip", // Original Java primary endpoint.
        "https://4.ipw.cn",
        "https://api.ipify.org",
        "https://ipv4.icanhazip.com",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
// The legacy parameter is retained for callers, but cannot supply a proxied detector.
pub async fn public_ip(_client: &reqwest::Client) -> Result<String> {
    public_ip_from(&detector_client()?, &sources()).await
}
async fn public_ip_from(client: &reqwest::Client, sources: &[String]) -> Result<String> {
    use futures_util::{StreamExt, stream};
    let distinct: std::collections::BTreeSet<_> = sources.iter().cloned().collect();
    ensure!(
        distinct.len() >= 2,
        "公网 IPv4 检测需要至少两个独立来源，未更新 DNS"
    );
    let requests: Vec<_> = distinct
        .into_iter()
        .map(|endpoint| fetch_address(client.clone(), endpoint))
        .collect();
    let mut replies = stream::iter(requests).buffer_unordered(4);
    let mut votes = BTreeMap::<std::net::Ipv4Addr, usize>::new();
    while let Some(reply) = replies.next().await {
        if let Some(ip) = reply {
            *votes.entry(ip).or_default() += 1;
        }
    }
    let mut ranked: Vec<_> = votes.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    match ranked.as_slice() {
        [(ip, count), rest @ ..]
            if *count >= 2 && rest.first().is_none_or(|(_, other)| count > other) =>
        {
            Ok(ip.to_string())
        }
        _ => anyhow::bail!("无法交叉确认公网 IPv4（来源不足或结果冲突），未更新 DNS"),
    }
}
async fn fetch_address(client: reqwest::Client, endpoint: String) -> Option<std::net::Ipv4Addr> {
    let mut response = client.get(endpoint).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 64 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_public_ipv4(&bytes)
}
fn parse_public_ipv4(bytes: &[u8]) -> Option<std::net::Ipv4Addr> {
    // These endpoints return a bare address. Never scrape IPv4 tokens from HTML/errors.
    let ip = std::str::from_utf8(bytes)
        .ok()?
        .trim()
        .parse::<std::net::Ipv4Addr>()
        .ok()?;
    let [a, b, c, _] = ip.octets();
    let reserved = a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && ((b == 0 && (c == 0 || c == 2)) || b == 168))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113);
    (!reserved).then_some(ip)
}

fn legacy_text<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Null => String::new(),
        Value::String(s) => s,
        v => v.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_webpage_addresses_and_non_public_ranges() {
        for body in [
            "<html>69.63.203.33</html>",
            "Current IP: 69.63.203.33",
            "69.63.203.33 8.8.8.8",
            "127.0.0.1",
            "10.1.2.3",
            "100.64.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "0.1.2.3",
            "::1",
            "999.1.2.3",
        ] {
            assert!(parse_public_ipv4(body.as_bytes()).is_none(), "{body}");
        }
        // Do not blacklist the reported address: its origin must be verified like any other.
        assert_eq!(
            parse_public_ipv4(b" 69.63.203.33\n").unwrap().to_string(),
            "69.63.203.33"
        );
    }
    #[tokio::test]
    async fn cross_checks_sources_and_rejects_invalid_or_ambiguous_responses() -> Result<()> {
        use axum::{Router, extract::Path as RoutePath, http::StatusCode, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = Router::new().route(
            "/{id}",
            get(|RoutePath(id): RoutePath<String>| async move {
                match id.as_str() {
                    "a" | "b" => (StatusCode::OK, "8.8.4.31\n".to_string()),
                    "wrong" | "wrong2" => (StatusCode::OK, "69.63.203.33".to_string()),
                    "html" => (StatusCode::OK, "<html>69.63.203.33</html>".to_string()),
                    "long" => (StatusCode::OK, format!("8.8.4.31{}", " ".repeat(128))),
                    "private" => (StatusCode::OK, "192.168.0.1".to_string()),
                    _ => (StatusCode::BAD_GATEWAY, "69.63.203.33".to_string()),
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = detector_client()?;
        let urls = |paths: &[&str]| {
            paths
                .iter()
                .map(|p| format!("http://{address}/{p}"))
                .collect::<Vec<_>>()
        };
        for paths in [
            vec!["a", "b", "wrong"],
            vec!["html", "a", "b"],
            vec!["long", "private", "error", "a", "b"],
        ] {
            assert_eq!(public_ip_from(&client, &urls(&paths)).await?, "8.8.4.31");
        }
        for paths in [
            vec!["a", "wrong"],
            vec!["a", "b", "wrong", "wrong2"],
            vec!["a", "a"],
            vec!["a", "html", "long", "private", "error"],
        ] {
            assert!(
                public_ip_from(&client, &urls(&paths)).await.is_err(),
                "{paths:?}"
            );
        }
        server.abort();
        Ok(())
    }
    #[tokio::test]
    async fn incomplete_response_cannot_vote_for_an_address() -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                let _ = socket.read(&mut request).await;
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n69.63.203.33").await.unwrap();
            }
        });
        assert!(
            public_ip_from(
                &detector_client()?,
                &[format!("http://{address}/a"), format!("http://{address}/b")]
            )
            .await
            .is_err()
        );
        server.await?;
        Ok(())
    }
}
