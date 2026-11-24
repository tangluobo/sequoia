//! Bounded recovery of transport failures. Only typed database reads are replayed.
use super::*;
use std::{fmt, io, time::Duration};
use tomato_core::workbench::{DatabaseAction, DatabaseOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Opening,
    Executing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    Preserved,
    Disconnected,
    Restored,
    Failed,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ConnectionFailure {
    pub connection_id: String,
    pub transaction_lost: bool,
    pub attempts: usize,
    pub read_only: bool,
    pub stage: FailureStage,
    pub recovery: RecoveryState,
}
impl ConnectionFailure {
    pub fn sessions_invalidated(&self) -> bool {
        self.recovery != RecoveryState::Preserved
    }
    pub fn is_restored(&self) -> bool {
        self.recovery == RecoveryState::Restored
    }
}
impl fmt::Display for ConnectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.transaction_lost {
            f.write_str(
                "数据库连接已中断，原事务已失效，未自动重新执行操作；请核对数据库中的提交结果",
            )
        } else if self.stage == FailureStage::Opening {
            write!(
                f,
                "数据库连接失败（已尝试 {} 次）；本条语句尚未发送到数据库",
                self.attempts
            )
        } else if self.recovery == RecoveryState::Restored {
            f.write_str("SSH 通道和数据库已重新连接；本条语句未自动重放，请核对执行结果")
        } else if self.recovery == RecoveryState::Failed {
            f.write_str("SSH 通道或数据库自动重连失败；本条语句未自动重放，请检查网络后重试")
        } else if !self.read_only {
            f.write_str("数据库连接已中断；本条语句未自动重放，请核对提交结果后重试")
        } else {
            write!(
                f,
                "数据库连接恢复失败（已尝试 {} 次），请检查网络后重试",
                self.attempts
            )
        }
    }
}
impl std::error::Error for ConnectionFailure {}

#[derive(Debug)]
struct RestoreFailure(anyhow::Error);
impl fmt::Display for RestoreFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "重新建立连接失败：{:#}", self.0)
    }
}
impl std::error::Error for RestoreFailure {}

pub(super) fn opening_failure(
    db: &mut Databases,
    connection: &Connection,
    error: anyhow::Error,
) -> anyhow::Error {
    let attempts = error
        .downcast_ref::<super::opening::Failure>()
        .map_or(1, |f| f.attempts);
    db.connection_error(connection, error, false, attempts)
}

pub(super) fn transport(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        if let Some(ssh) = cause.downcast_ref::<russh::Error>() {
            return match ssh {
                russh::Error::IO(error) => transport_io(error),
                russh::Error::Disconnect
                | russh::Error::HUP
                | russh::Error::ConnectionTimeout
                | russh::Error::KeepaliveTimeout
                | russh::Error::InactivityTimeout
                | russh::Error::SendError
                | russh::Error::Elapsed(_) => true,
                _ => false,
            };
        }
        if cause.is::<tokio::time::error::Elapsed>() {
            return true;
        }
        if let Some(io) = cause.downcast_ref::<io::Error>() {
            return transport_io(io);
        }
        if let Some(sqlx::Error::Database(db)) = cause.downcast_ref::<sqlx::Error>() {
            return db.code().is_some_and(|code| {
                code.starts_with("08")
                    || matches!(
                        code.as_ref(),
                        "2006" | "2013" | "2055" | "57P01" | "57P02" | "57P03"
                    )
            });
        }
        false
    })
}
fn transport_io(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::NotConnected
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::TimedOut
    ) || matches!(error.raw_os_error(), Some(10053 | 10054 | 10060))
}
fn read_only(action: &DatabaseAction) -> bool {
    matches!(
        action,
        DatabaseAction::Page { .. }
            | DatabaseAction::Definition { .. }
            | DatabaseAction::Design { .. }
            | DatabaseAction::DesignStructure { .. }
            | DatabaseAction::DesignSql { .. }
            | DatabaseAction::DesignInfo { .. }
            | DatabaseAction::DesignMetadata
            | DatabaseAction::Objects { .. }
            | DatabaseAction::ObjectHeaders { .. }
            | DatabaseAction::ObjectFields { .. }
            | DatabaseAction::Completion { .. }
            | DatabaseAction::Databases
            | DatabaseAction::Schemas
            | DatabaseAction::Tables { .. }
            | DatabaseAction::SelectSchema { .. }
    )
}
fn database<'a>(connection: &'a Connection, action: &'a DatabaseAction) -> &'a str {
    if connection.kind() != "MYSQL" {
        return connection.text("database");
    }
    match action {
        DatabaseAction::Objects { schema }
        | DatabaseAction::ObjectHeaders { schema }
        | DatabaseAction::ObjectFields { schema, .. }
        | DatabaseAction::ObjectMutation { schema, .. }
        | DatabaseAction::Completion { schema }
        | DatabaseAction::Tables { schema }
        | DatabaseAction::SelectSchema { schema } => schema,
        DatabaseAction::Page { request } | DatabaseAction::Apply { request, .. } => {
            &request.table.schema
        }
        DatabaseAction::Definition { table }
        | DatabaseAction::DeleteRows { table, .. }
        | DatabaseAction::Design { table }
        | DatabaseAction::DesignStructure { table }
        | DatabaseAction::DesignSql { table }
        | DatabaseAction::DesignInfo { table, .. } => &table.schema,
        DatabaseAction::SaveDesign { edit } | DatabaseAction::SaveStructure { edit } => {
            &edit.after.table.schema
        }
        DatabaseAction::CreateTable { request } => &request.table.schema,
        _ => connection.text("database"),
    }
}
impl Databases {
    fn connection_error(
        &mut self,
        connection: &Connection,
        error: anyhow::Error,
        read_only: bool,
        attempts: usize,
    ) -> anyhow::Error {
        let opening = error.downcast_ref::<super::opening::Failure>();
        let stage = if opening.is_some() {
            FailureStage::Opening
        } else {
            FailureStage::Executing
        };
        let preserved = opening.is_some_and(|f| f.preserved);
        let transaction_lost = !preserved && self.discard(&connection.id);
        error.context(ConnectionFailure {
            connection_id: connection.id.clone(),
            transaction_lost,
            attempts,
            read_only,
            stage,
            recovery: if preserved {
                RecoveryState::Preserved
            } else {
                RecoveryState::Disconnected
            },
        })
    }
    /// Normalize transport failures from operations outside the SQL/workbench adapters.
    pub(super) async fn recover_operation_error(
        &mut self,
        connection: &Connection,
        error: anyhow::Error,
    ) -> anyhow::Error {
        if error.downcast_ref::<ConnectionFailure>().is_some() || !transport(&error) {
            return error;
        }
        let error = self.connection_error(connection, error, false, 1);
        self.restore_after_error(connection, connection.text("database"), error)
            .await
    }
    /// Restore only the transport. Never send the failed user SQL again.
    pub(super) async fn restore_after_error(
        &mut self,
        connection: &Connection,
        database: &str,
        mut error: anyhow::Error,
    ) -> anyhow::Error {
        let eligible = connection.flag("useSshTunnel")
            && error.downcast_ref::<ConnectionFailure>().is_some_and(|f| {
                f.stage == FailureStage::Executing
                    && f.recovery == RecoveryState::Disconnected
                    && !f.transaction_lost
                    && !f.read_only
            });
        if !eligible {
            return error;
        }
        let restored = self.session_once(connection, database).await.map(|_| ());
        let state = if restored.is_ok() {
            RecoveryState::Restored
        } else {
            RecoveryState::Failed
        };
        if let Some(failure) = error.downcast_mut::<ConnectionFailure>() {
            failure.recovery = state;
        }
        if let Err(last) = restored {
            error = error.context(RestoreFailure(last));
        }
        error
    }

    /// An explicit profile location also keeps protocol fixtures out of the user's store.
    pub fn with_ssh_configuration(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            ssh_configuration: Some(path.into()),
            ..Self::default()
        }
    }
    fn discard(&mut self, id: &str) -> bool {
        let lost = self.transactions.iter().any(|key| key.connection == id);
        self.transactions.retain(|key| key.connection != id);
        // A broken connection must never wait for a graceful protocol close.
        self.sessions.retain(|key, _| key.connection != id);
        self.tunnels.remove(id);
        lost
    }
    pub async fn workbench(
        &mut self,
        connection: &Connection,
        action: DatabaseAction,
    ) -> Result<DatabaseOutput> {
        let key = SessionKey {
            connection: connection.id.clone(),
            database: database(connection, &action).into(),
        };
        let active = self
            .transactions
            .iter()
            .any(|key| key.connection == connection.id);
        let replay = read_only(&action) && !active;
        if !replay {
            let result = self.workbench_once(connection, action).await;
            return match result {
                Ok(output) => {
                    if let DatabaseOutput::Transaction(on) = &output {
                        if *on {
                            self.transactions.insert(key);
                        } else {
                            self.transactions.remove(&key);
                        }
                    }
                    Ok(output)
                }
                Err(error) if error.downcast_ref::<ConnectionFailure>().is_some() => Err(error),
                Err(error) if transport(&error) => {
                    let error = self.connection_error(connection, error, false, 1);
                    Err(self
                        .restore_after_error(connection, &key.database, error)
                        .await)
                }
                Err(error) => Err(error),
            };
        }
        for attempt in 1..=3 {
            let result =
                super::opening::single_attempt(self.workbench_once(connection, action.clone()))
                    .await;
            match result {
                Ok(value) => return Ok(value),
                Err(error) if transport(&error) => {
                    let preserved = error
                        .downcast_ref::<super::opening::Failure>()
                        .is_some_and(|f| f.preserved);
                    if attempt == 3 || preserved {
                        return Err(self.connection_error(connection, error, true, attempt));
                    }
                    self.discard(&connection.id);
                    tokio::time::sleep(Duration::from_millis(if attempt == 1 { 200 } else { 500 }))
                        .await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }
    pub async fn query(&mut self, connection: &Connection, sql: &str) -> Result<Vec<QueryResult>> {
        self.query_with_literal_mode(connection, sql, None).await
    }
    pub(super) async fn query_with_literal_mode(
        &mut self,
        connection: &Connection,
        sql: &str,
        literal_mode: Option<bool>,
    ) -> Result<Vec<QueryResult>> {
        self.query_with_recovery(connection, sql, literal_mode, true)
            .await
    }
    pub(super) async fn query_without_restore(
        &mut self,
        connection: &Connection,
        sql: &str,
        literal_mode: Option<bool>,
    ) -> Result<Vec<QueryResult>> {
        self.query_with_recovery(connection, sql, literal_mode, false)
            .await
    }
    async fn query_with_recovery(
        &mut self,
        connection: &Connection,
        sql: &str,
        literal_mode: Option<bool>,
        restore: bool,
    ) -> Result<Vec<QueryResult>> {
        match self.query_once(connection, sql).await {
            Ok(result) => {
                let key = SessionKey {
                    connection: connection.id.clone(),
                    database: connection.text("database").into(),
                };
                for statement in tomato_core::sql_script::split_for_session(
                    connection.kind(),
                    Some(sql),
                    literal_mode.unwrap_or(connection.kind() == "MYSQL"),
                ) {
                    if let Some(active) = transaction_change_for(connection.kind(), statement) {
                        if active {
                            self.transactions.insert(key.clone());
                        } else {
                            self.transactions.remove(&key);
                        }
                    }
                }
                Ok(result)
            }
            Err(error) if error.downcast_ref::<ConnectionFailure>().is_some() => Err(error),
            Err(error) if transport(&error) => {
                let error = self.connection_error(connection, error, false, 1);
                Err(if restore {
                    self.restore_after_error(connection, connection.text("database"), error)
                        .await
                } else {
                    error
                })
            }
            Err(error) => Err(error),
        }
    }
}
fn skip_comments(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if sql.starts_with("--") || sql.starts_with('#') {
            sql = sql.split_once('\n').map_or("", |(_, rest)| rest);
        } else if let Some(comment) = sql.strip_prefix("/*") {
            let bytes = comment.as_bytes();
            let mut depth = 1usize;
            let mut cursor = 0;
            while cursor + 1 < bytes.len() && depth > 0 {
                if &bytes[cursor..cursor + 2] == b"/*" {
                    depth += 1;
                    cursor += 2;
                } else if &bytes[cursor..cursor + 2] == b"*/" {
                    depth -= 1;
                    cursor += 2;
                } else {
                    cursor += 1;
                }
            }
            sql = if depth == 0 { &comment[cursor..] } else { "" };
        } else {
            break;
        }
    }
    sql
}
fn transaction_change_for(kind: &str, sql: &str) -> Option<bool> {
    if kind == "ORACLE" {
        let text = skip_comments(sql);
        if text
            .get(..5)
            .is_some_and(|head| head.eq_ignore_ascii_case("BEGIN"))
            && !skip_comments(&text[5..])
                .trim()
                .trim_end_matches(';')
                .trim()
                .is_empty()
        {
            // A PL/SQL BEGIN...END unit is not the adapter's bare BEGIN control.
            return None;
        }
    }
    transaction_change(sql)
}
fn transaction_change(mut sql: &str) -> Option<bool> {
    let mut next = || {
        sql = skip_comments(sql);
        let count = sql.bytes().take_while(u8::is_ascii_alphabetic).count();
        let word = &sql[..count];
        sql = &sql[count..];
        word
    };
    let head = next();
    if head.eq_ignore_ascii_case("BEGIN") {
        return Some(true);
    }
    if head.eq_ignore_ascii_case("START") {
        return next().eq_ignore_ascii_case("TRANSACTION").then_some(true);
    }
    if !["COMMIT", "END", "ROLLBACK"]
        .iter()
        .any(|word| head.eq_ignore_ascii_case(word))
    {
        return None;
    }
    let mut token = next();
    if token.eq_ignore_ascii_case("WORK") || token.eq_ignore_ascii_case("TRANSACTION") {
        token = next();
    }
    if head.eq_ignore_ascii_case("ROLLBACK") && token.eq_ignore_ascii_case("TO") {
        return None;
    }
    Some(token.eq_ignore_ascii_case("AND") && next().eq_ignore_ascii_case("CHAIN"))
}
#[cfg(test)]
mod tests;
