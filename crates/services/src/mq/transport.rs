use super::*;
use anyhow::Context;
use std::{collections::HashMap, time::Duration};
use tokio::net::TcpStream;
pub struct Transport {
    connection: Connection,
    tunnels: HashMap<String, crate::terminal::ssh::tunnel::Tunnel>,
}
impl Transport {
    pub fn new(c: &Connection) -> Self {
        Self {
            connection: c.clone(),
            tunnels: HashMap::new(),
        }
    }
    pub async fn dial(&mut self, address: &str) -> Result<TcpStream> {
        let url = url::Url::parse(&format!("tcp://{address}"))?;
        let host = url.host_str().context("消息服务器主机无效")?;
        let port = url.port().context("消息服务器端口无效")?;
        let address = if self.connection.flag("useSshTunnel") {
            if !self.tunnels.contains_key(address) {
                ensure!(self.tunnels.len() < 32, "消息集群 SSH 隧道数量超过 32");
                let mut target = self.connection.clone();
                target.set("host", host.trim_matches(['[', ']']));
                target.set("port", port);
                self.tunnels.insert(
                    address.into(),
                    crate::terminal::ssh::tunnel::Tunnel::open(&target).await?,
                );
            }
            self.tunnels[address].address.to_string()
        } else {
            address.into()
        };
        let socket =
            tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&address)).await??;
        socket.set_nodelay(true)?;
        Ok(socket)
    }
}
pub fn bootstrap(c: &Connection, separator: char) -> Vec<String> {
    let host = c.text("host");
    host.split(separator)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with('[') && s.contains("]:")
                || s.bytes().filter(|b| *b == b':').count() == 1
            {
                s.into()
            } else if s.contains(':') {
                format!("[{s}]:{}", c.number("port"))
            } else {
                format!("{s}:{}", c.number("port"))
            }
        })
        .collect()
}
