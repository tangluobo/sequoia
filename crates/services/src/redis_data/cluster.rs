//! Cluster discovery keeps original Redis/TLS identities and routes each node through SSH.
use super::*;
use crate::terminal::ssh::tunnel::{Forwarder, Tunnel};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

type Endpoint = (String, u16);
type Slot = Arc<OnceCell<Tunnel>>;
struct State {
    jump: Connection,
    known_hosts: PathBuf,
    routes: parking_lot::Mutex<BTreeMap<Endpoint, Slot>>,
    closed: CancellationToken,
    transport: parking_lot::Mutex<Option<Arc<OnceCell<Forwarder>>>>,
}
#[derive(Clone)]
pub(super) struct Resolver(Arc<State>);
pub(super) struct Lease(Resolver);
impl Lease {
    pub(super) fn resolver(&self) -> Resolver {
        self.0.clone()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.0.closed.cancel();
        self.0.0.routes.lock().clear();
        self.0.0.transport.lock().take();
    }
}
impl Resolver {
    pub(super) fn via(jump: Connection, known_hosts: PathBuf) -> Lease {
        Lease(Self(Arc::new(State {
            jump,
            known_hosts,
            routes: Default::default(),
            closed: CancellationToken::new(),
            transport: Default::default(),
        })))
    }
    async fn forward(&self, target: &Connection) -> Result<Tunnel> {
        let transport = {
            let mut slot = self.0.transport.lock();
            ensure!(!self.0.closed.is_cancelled(), "Redis 集群会话已关闭");
            if slot
                .as_ref()
                .and_then(|s| s.get())
                .is_some_and(|f| !f.is_active())
            {
                *slot = None;
            }
            slot.get_or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };
        transport
            .get_or_try_init(|| Forwarder::open(&self.0.jump, self.0.known_hosts.clone()))
            .await?
            .forward(target)
            .await
    }
    async fn address(&self, host: &str, port: u16) -> Result<SocketAddr> {
        ensure!(
            !host.is_empty() && host.len() <= 1024 && port > 0,
            "Redis 集群节点地址无效"
        );
        let endpoint = (host.to_owned(), port);
        let slot = {
            let mut routes = self.0.routes.lock();
            ensure!(!self.0.closed.is_cancelled(), "Redis 集群会话已关闭");
            if routes
                .get(&endpoint)
                .and_then(|slot| slot.get())
                .is_some_and(|t| !t.is_active())
            {
                routes.remove(&endpoint);
            }
            if !routes.contains_key(&endpoint) {
                ensure!(routes.len() < 128, "Redis 集群 SSH 节点超过 128 个");
                routes.insert(endpoint.clone(), Arc::new(OnceCell::new()));
            }
            routes[&endpoint].clone()
        };
        let opened = slot
            .get_or_try_init(|| async {
                let mut target = Connection::new("REDIS");
                target.set("host", host);
                target.set("port", port);
                tokio::select! {
                    biased;
                    _ = self.0.closed.cancelled() => bail!("Redis 集群会话已关闭"),
                    tunnel = self.forward(&target) => tunnel,
                }
            })
            .await;
        let tunnel = match opened {
            Ok(tunnel) => tunnel,
            Err(error) => {
                let mut routes = self.0.routes.lock();
                if Arc::strong_count(&slot) == 2
                    && routes
                        .get(&endpoint)
                        .is_some_and(|current| Arc::ptr_eq(current, &slot))
                {
                    routes.remove(&endpoint);
                }
                return Err(error);
            }
        };
        ensure!(!self.0.closed.is_cancelled(), "Redis 集群会话已关闭");
        Ok(tunnel.address)
    }
}
impl redis::io::AsyncDNSResolver for Resolver {
    fn resolve<'a, 'b: 'a>(
        &'a self,
        host: &'b str,
        port: u16,
    ) -> redis::RedisFuture<'a, Box<dyn Iterator<Item = SocketAddr> + Send + 'a>> {
        Box::pin(async move {
            let address = self.address(host, port).await.map_err(|error| {
                redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "Redis 集群 SSH 转发失败",
                    error.to_string(),
                ))
            })?;
            Ok(Box::new(std::iter::once(address)) as Box<dyn Iterator<Item = SocketAddr> + Send>)
        })
    }
}
pub(super) fn seeds(connection: &Connection) -> Result<Vec<String>> {
    let mut seeds = Vec::new();
    for node in connection
        .text("redisClusterNodes")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ensure!(seeds.len() < 128, "Redis 集群种子节点超过 128 个");
        let parsed = url::Url::parse(&format!("redis://{node}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("集群节点缺少主机"))?;
        let mut config = connection.clone();
        config.set("host", host);
        config.set("port", parsed.port().unwrap_or(6379));
        config.set("redisDatabase", 0);
        seeds.push(crate::network::redis_uri(&config)?.to_string());
    }
    ensure!(!seeds.is_empty(), "Redis 集群节点配置为空");
    Ok(seeds)
}
pub(super) async fn session(connection: &Connection) -> Result<Session> {
    session_at(connection, &tomato_core::Store::default_path(), None).await
}
async fn session_at(
    connection: &Connection,
    profile: &std::path::Path,
    certificates: Option<redis::TlsCertificates>,
) -> Result<Session> {
    // The library's one-second default also includes SSH authentication and
    // node discovery. Match the application connect budget instead.
    let mut client = redis::cluster::ClusterClient::builder(seeds(connection)?)
        .connection_timeout(std::time::Duration::from_secs(10))
        .response_timeout(std::time::Duration::from_secs(30));
    if let Some(certificates) = certificates {
        client = client.certs(certificates);
    }
    let routes = if connection.flag("useSshTunnel") {
        let (jump, known) = Tunnel::jump_at(connection, profile).await?;
        let lease = Resolver::via(jump, known);
        client = client.async_dns_resolver(lease.resolver());
        Some(lease)
    } else {
        None
    };
    Ok(Session::Cluster {
        stream: client.build()?.get_async_connection().await?,
        routes,
    })
}
pub(super) async fn node(connection: &Connection, resolver: Option<Resolver>) -> Result<Session> {
    let client = redis::Client::open(crate::network::redis_uri(connection)?.as_str())?;
    let mut config = redis::AsyncConnectionConfig::default()
        .set_connection_timeout(std::time::Duration::from_secs(10))
        .set_response_timeout(std::time::Duration::from_secs(30));
    if let Some(resolver) = resolver {
        config = config.set_dns_resolver(resolver);
    }
    Ok(Session::Single(
        client
            .get_multiplexed_async_connection_with_config(&config)
            .await?,
    ))
}

#[cfg(test)]
mod tests;
