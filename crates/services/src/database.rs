use anyhow::{Result, bail};
use futures_util::TryStreamExt;
use sqlx::{AnyConnection, Column, Connection as _, Row, ValueRef, any::AnyRow};
use std::collections::HashMap;
use tomato_core::Connection;

use tomato_core::limits;
mod admin;
mod backup;
pub mod export;
mod hive;
pub mod import;
mod objects;
mod opening;
mod oracle;
pub mod query_rows;
mod raw;
mod recovery;
pub use recovery::{ConnectionFailure, FailureStage, RecoveryState};
mod result;
pub mod script;
#[cfg(test)]
mod script_tests;
pub mod transfer;
mod workbench;
pub use result::{ColumnOrigin, QueryResult};
pub use workbench::inserts_sql;
pub use workbench::page_sql;

/// Native authentication decides whether the UI needs an unsaved password.
/// Invalid Hive options are reported by the connection attempt, not a password loop.
pub fn authentication_requires_password(connection: &Connection) -> bool {
    connection.kind() != "HIVE" || hive::requires_password(connection)
}

#[derive(Default)]
pub struct Databases {
    sessions: HashMap<SessionKey, AnyConnection>,
    tunnels: HashMap<String, crate::terminal::ssh::tunnel::Tunnel>,
    transactions: std::collections::HashSet<SessionKey>,
    ssh_configuration: Option<std::path::PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SessionKey {
    connection: String,
    database: String,
}

impl Databases {
    async fn query_once(&mut self, connection: &Connection, sql: &str) -> Result<Vec<QueryResult>> {
        let conn = self.session(connection).await?;
        let row_limit = query_row_limit(connection);
        if connection.kind() == "HIVE" {
            return conn
                .backend_mut::<hive::Backend>()
                .ok_or_else(|| anyhow::anyhow!("Hive 会话类型不匹配"))?
                .execute_raw(sql, row_limit)
                .await;
        }
        if connection.kind() == "ORACLE" {
            let owner = connection.text("__tomatoOracleOwner");
            if !owner.is_empty() {
                use sqlx::Executor;
                let quoted = crate::dialect::resolve("ORACLE")?.quote(owner)?;
                conn.execute(format!("ALTER SESSION SET CURRENT_SCHEMA={quoted}").as_str())
                    .await?;
            }
            return conn
                .backend_mut::<oracle::Backend>()
                .ok_or_else(|| anyhow::anyhow!("Oracle 会话类型不匹配"))?
                .raw(sql, row_limit)
                .await;
        }
        if connection.kind() == "MYSQL" {
            let native = conn
                .backend_mut::<sqlx::MySqlConnection>()
                .ok_or_else(|| anyhow::anyhow!("MySQL 原生会话类型不匹配"))?;
            return raw::mysql(native, sql, row_limit).await;
        }
        if connection.kind() == "POSTGRESQL" {
            let native = conn
                .backend_mut::<sqlx::PgConnection>()
                .ok_or_else(|| anyhow::anyhow!("PostgreSQL 原生会话类型不匹配"))?;
            return raw::postgres(native, sql, row_limit).await;
        }
        let mut stream = sqlx::raw_sql(sql).fetch_many(conn);
        let mut results = Vec::new();
        let mut current = QueryResult::default();
        let mut retained_bytes = 0usize;
        while let Some(item) = stream.try_next().await? {
            match item {
                sqlx::Either::Left(done) => {
                    current.affected = done.rows_affected();
                    if results.len() < limits::QUERY_RESULT_SETS {
                        results.push(std::mem::take(&mut current));
                    } else {
                        current = QueryResult::default();
                        if let Some(last) = results.last_mut() {
                            last.truncated = true;
                        }
                    }
                }
                sqlx::Either::Right(row) => {
                    current.has_result_set = true;
                    current.total_rows = current.total_rows.saturating_add(1);
                    if current.columns.is_empty() {
                        current.columns =
                            row.columns().iter().map(|c| c.name().to_string()).collect();
                    }
                    if current.rows.len() < row_limit
                        && retained_bytes < limits::QUERY_BYTES
                        && results.len() < limits::QUERY_RESULT_SETS
                    {
                        let values = (0..row.len())
                            .map(|i| cell(&row, i))
                            .collect::<Result<Vec<_>>>()?;
                        let bytes = current.row_bytes(&values);
                        if retained_bytes.saturating_add(bytes) <= limits::QUERY_BYTES {
                            retained_bytes += bytes;
                            current.push_row(values);
                        } else {
                            current.truncated = true;
                        }
                    } else {
                        current.truncated = true;
                    }
                }
            }
        }
        if !current.columns.is_empty() && results.len() < limits::QUERY_RESULT_SETS {
            results.push(current);
        }
        Ok(results)
    }

    pub async fn disconnect(&mut self, id: &str) -> Result<()> {
        self.transactions.retain(|key| key.connection != id);
        let tunnel = self.tunnels.remove(id);
        let keys = self
            .sessions
            .keys()
            .filter(|key| key.connection == id)
            .cloned()
            .collect::<Vec<_>>();
        let mut result = Ok(());
        for key in keys {
            if let Some(conn) = self.sessions.remove(&key)
                && let Err(error) = conn.close().await
                && result.is_ok()
            {
                result = Err(error.into());
            }
        }
        if let Some(tunnel) = tunnel {
            tunnel.close().await;
        }
        result
    }
    pub async fn disconnect_database(&mut self, id: &str, database: &str) -> Result<()> {
        let key = SessionKey {
            connection: id.into(),
            database: database.into(),
        };
        self.transactions.remove(&key);
        let result = if let Some(conn) = self.sessions.remove(&key) {
            conn.close().await.map_err(Into::into)
        } else {
            Ok(())
        };
        if !self.sessions.keys().any(|key| key.connection == id)
            && let Some(tunnel) = self.tunnels.remove(id)
        {
            tunnel.close().await;
        }
        result
    }
}

fn query_row_limit(connection: &Connection) -> usize {
    let limit =
        usize::try_from(connection.number("queryRowLimit")).unwrap_or(limits::DEFAULT_QUERY_ROWS);
    if limit == 0 {
        limits::DEFAULT_QUERY_ROWS
    } else {
        limit.min(limits::MAX_QUERY_ROWS)
    }
}
fn cell(row: &AnyRow, index: usize) -> Result<Option<String>> {
    if row.try_get_raw(index)?.is_null() {
        return Ok(None);
    }
    if let Ok(value) = row.try_get::<String, _>(index) {
        return Ok(Some(value));
    }
    if let Ok(value) = row.try_get::<i64, _>(index) {
        return Ok(Some(value.to_string()));
    }
    if let Ok(value) = row.try_get::<i32, _>(index) {
        return Ok(Some(value.to_string()));
    }
    if let Ok(value) = row.try_get::<f64, _>(index) {
        return Ok(Some(value.to_string()));
    }
    if let Ok(value) = row.try_get::<bool, _>(index) {
        return Ok(Some(value.to_string()));
    }
    if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
        return Ok(Some(
            value.iter().map(|byte| format!("{byte:02x}")).collect(),
        ));
    }
    bail!(
        "列 {} 的类型暂不支持，请在 SQL 中 CAST 为文本",
        row.columns()[index].name()
    )
}

pub fn connection_uri(connection: &Connection) -> Result<String> {
    connection_uri_in(connection, connection.text("database"))
}
fn connection_uri_in(connection: &Connection, database: &str) -> Result<String> {
    if connection.kind() == "SQLITE" {
        let path = database;
        if path.is_empty() || path == ":memory:" {
            return Ok("sqlite::memory:".into());
        }
        let encoded =
            percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC);
        return Ok(format!("sqlite://{encoded}?mode=rwc"));
    }
    let scheme = match connection.kind() {
        "MYSQL" => "mysql",
        "POSTGRESQL" => "postgres",
        other => bail!("{other} 的数据库适配器尚未迁移"),
    };
    let mut uri = url::Url::parse(&format!("{scheme}://localhost"))?;
    uri.set_host(Some(connection.text("host")))
        .map_err(|_| anyhow::anyhow!("无效主机"))?;
    let port = u16::try_from(connection.number("port"))?;
    uri.set_port(Some(port))
        .map_err(|_| anyhow::anyhow!("无效端口"))?;
    uri.set_username(connection.text("username"))
        .map_err(|_| anyhow::anyhow!("无效用户名"))?;
    uri.set_password(Some(connection.text("password")))
        .map_err(|_| anyhow::anyhow!("无效密码"))?;
    uri.set_path(&format!("/{database}"));
    if scheme == "postgres" {
        uri.query_pairs_mut().append_pair(
            "sslmode",
            if connection.flag("databaseTls") {
                "require"
            } else {
                "prefer"
            },
        );
    }
    Ok(uri.into())
}

pub fn tables_sql(kind: &str) -> &'static str {
    crate::dialect::resolve(kind)
        .map(|dialect| dialect.tables())
        .unwrap_or("SELECT 1")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn sqlite_multi_statement_and_transaction_share_session() -> Result<()> {
        let mut db = Databases::default();
        let connection = Connection::new("SQLITE");
        db.query(&connection, "CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES(1,'中文; value');").await?;
        db.query(
            &connection,
            "BEGIN; UPDATE t SET name='rollback'; ROLLBACK;",
        )
        .await?;
        let results = db
            .query(&connection, "SELECT * FROM t; SELECT COUNT(*) AS n FROM t;")
            .await?;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].rows[0], vec!["1", "中文; value"]);
        assert_eq!(results[1].rows[0][0], "1");
        Ok(())
    }
    #[test]
    fn credentials_are_url_encoded() -> Result<()> {
        let mut connection = Connection::new("MYSQL");
        connection.set("username", "user@host");
        connection.set("password", "p@ss:/?#");
        let uri = connection_uri(&connection)?;
        assert!(uri.contains("user%40host"));
        assert!(!uri.contains("p@ss:/?#"));
        Ok(())
    }

    #[test]
    fn legacy_rdp_ssl_default_does_not_force_postgres_tls() -> Result<()> {
        let mut connection = Connection::new("POSTGRESQL");
        connection.set("useSsl", true);
        assert!(connection_uri(&connection)?.contains("sslmode=prefer"));
        connection.set("databaseTls", true);
        assert!(connection_uri(&connection)?.contains("sslmode=require"));
        Ok(())
    }

    #[tokio::test]
    async fn result_limits_do_not_skip_later_statements() -> Result<()> {
        let mut db = Databases::default();
        let mut connection = Connection::new("SQLITE");
        connection.set("queryRowLimit", 2);
        let results=db.query(&connection,"CREATE TABLE t(id INTEGER); INSERT INTO t VALUES(1),(2),(3); SELECT id FROM t; INSERT INTO t VALUES(4);").await?;
        assert_eq!(results[2].rows.len(), 2);
        assert!(results[2].truncated);
        let count = db.query(&connection, "SELECT COUNT(*) FROM t;").await?;
        assert_eq!(count[0].rows[0][0], "4");
        Ok(())
    }

    #[tokio::test]
    async fn sqlite_file_paths_preserve_spaces_and_unicode() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("数据库 with spaces.db");
        let mut connection = Connection::new("SQLITE");
        connection.set("database", path.to_string_lossy().to_string());
        let mut db = Databases::default();
        db.query(
            &connection,
            "CREATE TABLE t(id INTEGER); INSERT INTO t VALUES(42);",
        )
        .await?;
        db.disconnect(&connection.id).await?;
        assert!(path.exists());
        let result = db.query(&connection, "SELECT id FROM t;").await?;
        assert_eq!(result[0].rows[0][0], "42");
        Ok(())
    }
    #[tokio::test]
    async fn same_connection_id_keeps_database_sessions_transactions_and_disconnect_isolated()
    -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut first = Connection::new("SQLITE");
        first.set(
            "database",
            directory
                .path()
                .join("first.db")
                .to_string_lossy()
                .to_string(),
        );
        let mut second = first.clone();
        second.set(
            "database",
            directory
                .path()
                .join("second.db")
                .to_string_lossy()
                .to_string(),
        );
        let mut db = Databases::default();
        db.query(
            &first,
            "CREATE TABLE only_first(id INTEGER); INSERT INTO only_first VALUES(1)",
        )
        .await?;
        assert_eq!(
            db.query(
                &second,
                "SELECT COUNT(*) FROM sqlite_master WHERE name='only_first'"
            )
            .await?[0]
                .rows[0][0],
            "0"
        );
        db.query(
            &second,
            "CREATE TABLE only_second(id INTEGER); INSERT INTO only_second VALUES(2)",
        )
        .await?;
        db.query(&first, "BEGIN; INSERT INTO only_first VALUES(3)")
            .await?;
        assert_eq!(
            db.query(&second, "SELECT id FROM only_second").await?[0].rows[0][0],
            "2"
        );
        assert_eq!(
            db.query(
                &first,
                "SELECT COUNT(*) FROM sqlite_master WHERE name='only_second'"
            )
            .await?[0]
                .rows[0][0],
            "0"
        );
        db.query(&first, "ROLLBACK").await?;
        assert_eq!(
            db.query(&first, "SELECT COUNT(*) FROM only_first").await?[0].rows[0][0],
            "1"
        );
        db.disconnect(&first.id).await?;
        directory.close()?;
        Ok(())
    }
}
