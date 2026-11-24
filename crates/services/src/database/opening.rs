//! Connection acquisition is separate from executing user SQL.
use super::{Databases, SessionKey, hive, oracle, recovery};
use anyhow::Result;
use sqlx::{AnyConnection, Connection as _};
use std::{fmt, future::Future, time::Duration};
use tomato_core::Connection;

tokio::task_local! { static SINGLE_ATTEMPT: (); }

/// A typed read already owns its retry budget; do not multiply it while opening.
pub(super) async fn single_attempt<T>(future: impl Future<Output = T>) -> T {
    SINGLE_ATTEMPT.scope((), future).await
}

type Opened = (AnyConnection, Option<crate::terminal::ssh::tunnel::Tunnel>);

#[derive(Clone, Copy)]
pub(super) enum Policy {
    Once,
    RetrySsh,
}

#[derive(Debug)]
pub(super) struct Failure {
    pub attempts: usize,
    pub preserved: bool,
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "建立数据库连接失败（已尝试 {} 次）", self.attempts)
    }
}
impl std::error::Error for Failure {}

impl Databases {
    pub(super) async fn session(&mut self, connection: &Connection) -> Result<&mut AnyConnection> {
        self.session_in(connection, connection.text("database"))
            .await
    }
    pub(super) async fn session_in(
        &mut self,
        connection: &Connection,
        database: &str,
    ) -> Result<&mut AnyConnection> {
        self.session_with_policy(connection, database, Policy::RetrySsh)
            .await
    }
    pub(super) async fn session_once(
        &mut self,
        connection: &Connection,
        database: &str,
    ) -> Result<&mut AnyConnection> {
        self.session_with_policy(connection, database, Policy::Once)
            .await
    }
    async fn session_with_policy(
        &mut self,
        connection: &Connection,
        database: &str,
        policy: Policy,
    ) -> Result<&mut AnyConnection> {
        crate::init_crypto();
        let key = SessionKey {
            connection: connection.id.clone(),
            database: if connection.kind() == "ORACLE" {
                connection.text("database")
            } else if connection.kind() == "HIVE" {
                hive::database_name(connection, database)
            } else {
                database
            }
            .into(),
        };
        if !self.sessions.contains_key(&key) {
            // Do not drop a different database session or a live transaction
            // while opening a new namespace for the same connection ID.
            let preserved = self
                .sessions
                .keys()
                .any(|key| key.connection == connection.id);
            let active = self
                .transactions
                .iter()
                .any(|key| key.connection == connection.id);
            let scoped = SINGLE_ATTEMPT.try_with(|_| ()).is_ok();
            let attempts = if !scoped
                && matches!(policy, Policy::RetrySsh)
                && connection.flag("useSshTunnel")
                && !preserved
                && !active
            {
                2
            } else {
                1
            };
            for attempt in 1..=attempts {
                match self.open_once(connection, database).await {
                    Ok((opened, tunnel)) => {
                        self.sessions.insert(key.clone(), opened);
                        if let Some(tunnel) = tunnel {
                            self.tunnels.insert(connection.id.clone(), tunnel);
                        }
                        break;
                    }
                    Err(error) if recovery::transport(&error) => {
                        if attempt < attempts {
                            self.tunnels.remove(&connection.id);
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        } else {
                            let error = error.context(Failure {
                                attempts: attempt,
                                preserved,
                            });
                            return Err(if !scoped && matches!(policy, Policy::RetrySsh) {
                                recovery::opening_failure(self, connection, error)
                            } else {
                                error
                            });
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        self.sessions
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("数据库会话不存在"))
    }
    async fn open_once(&mut self, connection: &Connection, database: &str) -> Result<Opened> {
        sqlx::any::install_default_drivers();
        let tunnel =
            if connection.flag("useSshTunnel") && !self.tunnels.contains_key(&connection.id) {
                Some(
                    crate::terminal::ssh::tunnel::Tunnel::open_at(
                        connection,
                        &self
                            .ssh_configuration
                            .clone()
                            .unwrap_or_else(tomato_core::Store::default_path),
                    )
                    .await?,
                )
            } else {
                None
            };
        let routed = if connection.flag("useSshTunnel") {
            self.tunnels
                .get(&connection.id)
                .or(tunnel.as_ref())
                .map(|t| t.routed(connection))
        } else {
            None
        };
        let opened = if connection.kind() == "ORACLE" {
            oracle::connect(routed.as_ref().unwrap_or(connection)).await?
        } else if connection.kind() == "HIVE" {
            hive::connect(connection, database, routed.as_ref()).await?
        } else {
            let uri = super::connection_uri_in(routed.as_ref().unwrap_or(connection), database)?;
            tokio::time::timeout(Duration::from_secs(10), AnyConnection::connect(&uri)).await??
        };
        Ok((opened, tunnel))
    }
}
