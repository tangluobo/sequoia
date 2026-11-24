//! Typed Redis key operations. Mutations are single-key atomic Lua transactions.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use tomato_core::Connection;
mod cluster;
mod delete;
use delete::delete_keys;
#[cfg(test)]
#[path = "redis_data/tunnel_tests.rs"]
mod tunnel_tests;
const BYTES: usize = 8 * 1024 * 1024;
const ITEMS: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Databases,
    Scan {
        pattern: String,
        cursor: u64,
    },
    Read {
        key: String,
    },
    Write {
        key: String,
        kind: String,
        text: String,
        create: bool,
    },
    Delete {
        keys: Vec<String>,
    },
    Rename {
        key: String,
        name: String,
    },
    Ttl {
        key: String,
        seconds: i64,
    },
}
#[derive(Debug, Serialize)]
pub enum Output {
    Databases(Vec<u32>),
    Keys {
        keys: Vec<String>,
        cursor: u64,
    },
    Value {
        key: String,
        kind: String,
        text: String,
        ttl: i64,
    },
    Changed {
        key: Option<String>,
        message: String,
    },
    DeleteReport {
        succeeded: usize,
        failed: usize,
        failures: Vec<(String, String)>,
    },
}
impl Action {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::Databases => 0,
            Self::Scan { pattern, .. } => pattern.capacity(),
            Self::Read { key } | Self::Ttl { key, .. } => key.capacity(),
            Self::Write {
                key, kind, text, ..
            } => key.capacity() + kind.capacity() + text.capacity(),
            Self::Delete { keys } => keys.iter().map(String::capacity).sum(),
            Self::Rename { key, name } => key.capacity() + name.capacity(),
        }
    }
}
pub fn write_args(kind: &str, text: &str) -> Result<Vec<String>> {
    ensure!(text.len() <= BYTES, "Redis 编辑内容超过 8 MiB");
    let mut args = Vec::new();
    match kind {
        "string" => args.push(text.to_owned()),
        "list" | "set" => {
            let mut lines: Vec<_> = text.split('\n').collect();
            while lines.len() > 1 && lines.last() == Some(&"") {
                lines.pop();
            }
            args.extend(lines.into_iter().map(str::to_owned));
        }
        "hash" | "zset" => {
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let (a, b) = line
                    .split_once(' ')
                    .ok_or_else(|| anyhow::anyhow!("每行需要空格分隔的两个字段"))?;
                if kind == "zset" {
                    let n: f64 = a.parse()?;
                    ensure!(n.is_finite(), "Score 必须是有限数字");
                }
                args.push(a.into());
                args.push(b.into());
            }
        }
        _ => bail!("此类型不能编辑：{kind}"),
    }
    ensure!(args.len() <= ITEMS * 2, "Redis 编辑元素过多");
    Ok(args)
}
async fn query<T: redis::FromRedisValue>(stream: &mut Session, cmd: &mut redis::Cmd) -> Result<T> {
    Ok(match stream {
        Session::Single(stream) => cmd.query_async(stream).await?,
        Session::Cluster { stream, .. } => cmd.query_async(stream).await?,
        Session::Routed { stream, .. } => cmd.query_async(stream).await?,
    })
}
enum Session {
    Single(redis::aio::MultiplexedConnection),
    Cluster {
        stream: redis::cluster_async::ClusterConnection,
        routes: Option<cluster::Lease>,
    },
    Routed {
        stream: redis::aio::MultiplexedConnection,
        _driver: Driver,
    },
}
struct Driver(tokio::task::JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn routed_session(
    connection: &Connection,
    address: Option<std::net::SocketAddr>,
) -> Result<Session> {
    let Some(address) = address else {
        return session(connection).await;
    };
    routed_session_at(connection, address, None).await
}
async fn routed_session_at(
    connection: &Connection,
    address: std::net::SocketAddr,
    tls: Option<rustls::ClientConfig>,
) -> Result<Session> {
    let client = redis::Client::open(crate::network::redis_uri(connection)?.as_str())?;
    let socket = tokio::net::TcpStream::connect(address).await?;
    socket.set_nodelay(true)?;
    let (stream, driver) = if connection.flag("redisTls") {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = tls.unwrap_or_else(|| {
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        });
        let name = rustls::pki_types::ServerName::try_from(connection.text("host").to_owned())?;
        let socket = tokio_rustls::TlsConnector::from(Arc::new(tls))
            .connect(name, socket)
            .await?;
        let (stream, driver) =
            redis::aio::MultiplexedConnection::new(&client.get_connection_info().redis, socket)
                .await?;
        (stream, Driver(tokio::spawn(driver)))
    } else {
        let (stream, driver) =
            redis::aio::MultiplexedConnection::new(&client.get_connection_info().redis, socket)
                .await?;
        (stream, Driver(tokio::spawn(driver)))
    };
    Ok(Session::Routed {
        stream,
        _driver: driver,
    })
}
async fn session(connection: &Connection) -> Result<Session> {
    if connection.flag("redisCluster") {
        cluster::session(connection).await
    } else {
        let client = redis::Client::open(crate::network::redis_uri(connection)?.as_str())?;
        Ok(Session::Single(
            client.get_multiplexed_async_connection().await?,
        ))
    }
}
async fn cluster_scan(
    stream: &mut Session,
    connection: &Connection,
    pattern: String,
) -> Result<Output> {
    let slots: Vec<redis::Value> = query(stream, redis::cmd("CLUSTER").arg("SLOTS")).await?;
    let mut masters = BTreeSet::new();
    for slot in slots {
        let row: Vec<redis::Value> = redis::from_redis_value(&slot)?;
        let primary = row
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("集群主节点响应不完整"))?;
        let address: Vec<redis::Value> = redis::from_redis_value(primary)?;
        ensure!(address.len() >= 2, "集群主节点地址不完整");
        let host: String = redis::from_redis_value(&address[0])?;
        let port: u16 = redis::from_redis_value(&address[1])?;
        ensure!(masters.len() < 1024, "Redis 集群主节点过多");
        masters.insert((host, port));
    }
    let mut keys = BTreeSet::new();
    let mut bytes = 0usize;
    let resolver = match stream {
        Session::Cluster {
            routes: Some(routes),
            ..
        } => Some(routes.resolver()),
        _ => None,
    };
    for (host, port) in masters {
        let mut config = connection.clone();
        config.set("redisCluster", false);
        config.set("redisDatabase", 0);
        config.set("host", host);
        config.set("port", port);
        let mut master = cluster::node(&config, resolver.clone()).await?;
        let mut cursor = 0u64;
        loop {
            let (next, batch): (u64, Vec<String>) = query(
                &mut master,
                redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(500),
            )
            .await?;
            for key in batch {
                if !keys.contains(&key) {
                    bytes = bytes.saturating_add(key.len());
                    keys.insert(key);
                }
            }
            ensure!(
                keys.len() <= ITEMS && bytes <= BYTES,
                "集群搜索超过 10000 个 Key 或 8 MiB，请缩小搜索范围"
            );
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
    }
    Ok(Output::Keys {
        keys: keys.into_iter().collect(),
        cursor: 0,
    })
}
pub async fn raw_command(connection: &Connection, args: &[String]) -> Result<String> {
    ensure!(!args.is_empty(), "请输入命令及参数");
    crate::init_crypto();
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let tunnel = route(connection).await?;
        let mut stream = routed_session(connection, tunnel.as_ref().map(|t| t.address)).await?;
        let mut cmd = redis::cmd(&args[0]);
        cmd.arg(&args[1..]);
        let value: redis::Value = query(&mut stream, &mut cmd).await?;
        Ok(format!("{value:#?}"))
    })
    .await?
}
fn append(text: &mut String, line: &str) -> Result<()> {
    ensure!(
        text.len().saturating_add(line.len() + 1) <= BYTES,
        "Redis 值超过 8 MiB 编辑上限"
    );
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(line);
    Ok(())
}
pub async fn execute(connection: &Connection, action: Action) -> Result<Output> {
    crate::init_crypto();
    let tunnel = route(connection).await?;
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        routed_session(connection, tunnel.as_ref().map(|t| t.address)),
    )
    .await??;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        match action {
            Action::Databases=> {
                if connection.flag("redisCluster") {return Ok(Output::Databases(vec![0]));}
                let settings:Result<Vec<String>>=query(&mut stream,redis::cmd("CONFIG").arg("GET").arg("databases")).await;
                let count=settings.ok().and_then(|v|v.get(1).and_then(|s|s.parse::<u32>().ok())).unwrap_or(16);
                ensure!(count<=10000,"Redis 数据库超过 10000 个");
                Ok(Output::Databases((0..count).collect()))
            }
            Action::Scan { pattern, cursor } => {
                let pattern = if pattern.trim().is_empty() { "*".to_owned() } else if !pattern.contains('*') {format!("*{pattern}*")} else {pattern};
                if connection.flag("redisCluster") { return cluster_scan(&mut stream,connection,pattern).await; }
                let (cursor, keys): (u64, Vec<String>) = query(&mut stream, redis::cmd("SCAN").arg(cursor).arg("MATCH").arg(pattern).arg("COUNT").arg(500)).await?;
                ensure!(keys.len() <= ITEMS && keys.iter().map(String::len).sum::<usize>() <= BYTES, "Redis Key 批次过大");
                Ok(Output::Keys { keys, cursor })
            }
            Action::Read { key } => {
                let kind: String = query(&mut stream, redis::cmd("TYPE").arg(&key)).await?;
                let ttl: i64 = query(&mut stream, redis::cmd("TTL").arg(&key)).await?;
                let mut text = String::new();
                match kind.as_str() {
                    "none" => {},
                    "string" => {
                        let bytes: usize = query(&mut stream, redis::cmd("STRLEN").arg(&key)).await?;
                        ensure!(bytes <= BYTES, "Redis 字符串超过 8 MiB 编辑上限");
                        text = query(&mut stream, redis::cmd("GETRANGE").arg(&key).arg(0).arg(BYTES)).await?;
                        ensure!(text.len() <= BYTES, "读取期间 Redis 字符串增长超过上限");
                    }
                    "list" => {
                        let count: usize = query(&mut stream, redis::cmd("LLEN").arg(&key)).await?;
                        ensure!(count <= ITEMS, "List 超过 10000 项，不能完整编辑");
                        for start in (0..count).step_by(100) {
                            let values: Vec<String> = query(&mut stream, redis::cmd("LRANGE").arg(&key).arg(start).arg((start+99).min(count-1))).await?;
                            for value in values { append(&mut text, &value)?; }
                        }
                    }
                    "set" | "hash" | "zset" => {
                        let mut cursor = 0u64; let mut seen = BTreeSet::new();
                        loop {
                            let command = match kind.as_str() { "set" => "SSCAN", "hash" => "HSCAN", _ => "ZSCAN" };
                            let (next, values): (u64, Vec<String>) = query(&mut stream, redis::cmd(command).arg(&key).arg(cursor).arg("COUNT").arg(100)).await?;
                            let stride = if kind == "set" { 1 } else { 2 };
                            for pair in values.chunks(stride) {
                                ensure!(pair.len() == stride, "Redis 扫描响应不完整");
                                if seen.insert(pair[0].clone()) {
                                    ensure!(seen.len() <= ITEMS, "Redis 集合超过 10000 项，不能完整编辑");
                                    let line = match kind.as_str() { "hash" => format!("{} {}", pair[0],pair[1]), "zset" => format!("{} {}",pair[1],pair[0]), _ => pair[0].clone() };
                                    append(&mut text,&line)?;
                                }
                            }
                            cursor = next; if cursor == 0 { break; }
                        }
                    }
                    "stream" => {
                        let value: redis::Value = query(&mut stream, redis::cmd("XRANGE").arg(&key).arg("-").arg("+").arg("COUNT").arg(100)).await?;
                        text = format!("{value:#?}"); ensure!(text.len() <= BYTES, "Stream 预览超过 8 MiB");
                    }
                    _ => bail!("不支持的 Redis 类型：{kind}"),
                }
                Ok(Output::Value { key, kind, text, ttl })
            }
            Action::Write { key, kind, text, create } => {
                ensure!(!key.is_empty(), "Key 不能为空");
                let args = write_args(&kind, &text)?;
                // Validate all payloads before DEL. Preserve millisecond expiration atomically.
                let script = "if ARGV[2]=='1' and redis.call('EXISTS',KEYS[1])==1 then return redis.error_reply('Key already exists') end local ttl=redis.call('PTTL',KEYS[1]); local kind=ARGV[1]; redis.call('DEL',KEYS[1]); if kind=='string' then redis.call('SET',KEYS[1],ARGV[3]); else local cmd=({list='RPUSH',set='SADD',hash='HSET',zset='ZADD'})[kind]; for i=3,#ARGV,(kind=='hash' or kind=='zset') and 2 or 1 do if kind=='hash' or kind=='zset' then redis.call(cmd,KEYS[1],ARGV[i],ARGV[i+1]); else redis.call(cmd,KEYS[1],ARGV[i]); end end end if ttl>=0 then redis.call('PEXPIRE',KEYS[1],math.max(ttl,1)); end return 1";
                let _: i64 = query(&mut stream, redis::cmd("EVAL").arg(script).arg(1).arg(&key).arg(&kind).arg(if create { "1" } else { "0" }).arg(args)).await?;
                Ok(Output::Changed { key: Some(key), message: "Key 已保存".into() })
            }
            Action::Delete { keys } => {
                ensure!(!keys.is_empty() && keys.len() <= ITEMS, "请选择要删除的 Key");
                delete_keys(&mut stream, keys).await
            }
            Action::Rename { key, name } => {
                ensure!(!name.is_empty(), "Key 不能为空");
                let _: String = query(&mut stream, redis::cmd("RENAME").arg(&key).arg(&name)).await?;
                Ok(Output::Changed { key: Some(name), message: "Key 已重命名".into() })
            }
            Action::Ttl { key, seconds } => {
                ensure!(seconds >= -1, "TTL 输入 -1 表示永久，其余须为非负秒数");
                let changed: i64 = if seconds == -1 { query(&mut stream, redis::cmd("PERSIST").arg(&key)).await? }
                    else { query(&mut stream, redis::cmd("EXPIRE").arg(&key).arg(seconds)).await? };
                Ok(Output::Changed { key: Some(key), message: if changed == 0 { "TTL 未改变（Key 不存在或已永久）" } else { "TTL 已更新" }.into() })
            }
        }
    }).await?
}

async fn route(connection: &Connection) -> Result<Option<crate::terminal::ssh::tunnel::Tunnel>> {
    if !connection.flag("useSshTunnel") || connection.flag("redisCluster") {
        return Ok(None);
    }
    Ok(Some(
        crate::terminal::ssh::tunnel::Tunnel::open(connection).await?,
    ))
}
