//! Scoped native TCP forwarding through the same verified SSH authentication stack.
use super::*;
use std::net::SocketAddr;
mod route;
mod shared;
pub use route::Route;
pub(crate) use shared::Forwarder;

pub struct Tunnel {
    pub address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    _connection: Forwarder,
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Tunnel {
    /// Normal asynchronous owners wait until the listener has actually closed.
    /// Drop remains nonblocking when a UI/session is cancelled abruptly.
    pub async fn close(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
    /// Compatibility with connections saved before reference-style jump hosts.
    pub fn inline_jump(target: &Connection) -> Option<Connection> {
        if !target.text("sshTunnelHostId").is_empty()
            || target.text("sshTunnelHost").trim().is_empty()
        {
            return None;
        }
        let mut jump = Connection::new("SSH");
        jump.id = format!("legacy:{}", target.id);
        jump.set("host", target.text("sshTunnelHost"));
        jump.set(
            "port",
            match target.number("sshTunnelPort") {
                0 => 22,
                port => port,
            },
        );
        jump.set("username", target.text("sshTunnelUsername"));
        jump.set("password", target.text("sshTunnelPassword"));
        jump.set(
            "usePassword",
            target
                .options
                .get("sshTunnelUsePassword")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
        );
        jump.set("useKey", target.flag("sshTunnelUseKey"));
        if let Some(paths) = target.options.get("sshTunnelPrivateKeyPaths") {
            jump.set("privateKeyPaths", paths.clone());
        }
        Some(jump)
    }
    pub async fn open(target: &Connection) -> Result<Self> {
        let config_path = tomato_core::Store::default_path();
        Self::open_at(target, &config_path).await
    }
    pub async fn open_at(target: &Connection, config_path: &std::path::Path) -> Result<Self> {
        let (jump, known_hosts) = Self::jump_at(target, config_path).await?;
        Self::open_via(target, &jump, known_hosts).await
    }
    /// Resolve a scoped jump snapshot once, without retaining the whole store
    /// while authenticating or opening discovered cluster nodes.
    pub(crate) async fn jump_at(
        target: &Connection,
        config_path: &std::path::Path,
    ) -> Result<(Connection, PathBuf)> {
        if let Some(jump) = Self::inline_jump(target) {
            return Ok((jump, config_path.with_file_name("known_hosts")));
        }
        let path = config_path.to_path_buf();
        let store = tokio::task::spawn_blocking(move || tomato_core::Store::load(&path)).await??;
        let id = target.text("sshTunnelHostId");
        ensure!(!id.is_empty(), "缺少引用的 SSH 跳板连接");
        let jump = store
            .connections
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .context("找不到引用的 SSH 跳板连接")?;
        Ok((jump, config_path.with_file_name("known_hosts")))
    }
    pub(crate) fn is_active(&self) -> bool {
        !self.task.is_finished()
    }
    /// Explicit jump/known-host paths also allow isolated protocol fixtures.
    pub async fn open_via(
        target: &Connection,
        jump: &Connection,
        known_hosts: PathBuf,
    ) -> Result<Self> {
        ensure!(
            !target.text("host").is_empty() && (1..=65535).contains(&target.number("port")),
            "隧道目标地址无效"
        );
        Forwarder::open(jump, known_hosts)
            .await?
            .forward(target)
            .await
    }
    pub fn routed(&self, target: &Connection) -> Connection {
        let mut connection = target.clone();
        connection.set("host", "127.0.0.1");
        connection.set("port", self.address.port());
        connection.set("useSshTunnel", false);
        connection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_legacy_jump_keeps_database_and_ssh_credentials_separate() {
        let mut target = Connection::new("MONGODB");
        target.set("host", "db.internal");
        target.set("password", "database-secret");
        target.set("sshTunnelHost", "jump.internal");
        target.set("sshTunnelUsername", "gateway-user");
        target.set("sshTunnelPassword", "key-passphrase");
        target.set("sshTunnelUsePassword", false);
        target.set("sshTunnelUseKey", true);
        target.set(
            "sshTunnelPrivateKeyPaths",
            serde_json::json!(["F:/fixture/key.pem"]),
        );
        let before = target.options.clone();
        let jump = Tunnel::inline_jump(&target).unwrap();
        assert_eq!(jump.text("host"), "jump.internal");
        assert_eq!(jump.number("port"), 22);
        assert_eq!(jump.text("password"), "key-passphrase");
        assert!(jump.flag("useKey"));
        assert!(!jump.flag("usePassword"));
        assert_eq!(
            jump.options["privateKeyPaths"],
            serde_json::json!(["F:/fixture/key.pem"])
        );
        assert_eq!(target.options, before);
        target.set("sshTunnelHostId", "reference");
        assert!(Tunnel::inline_jump(&target).is_none());
    }
}
