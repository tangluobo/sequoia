//! Native HiveServer2 behind the same streaming session facade as SQL databases.
mod bind;
pub(super) mod catalog;
mod client;
mod config;
mod data;
pub(super) mod designer;
mod http;
mod sasl;
#[cfg(test)]
mod tests;
mod tls_identity;
mod transport;
mod wire;
use anyhow::Result;
use futures_util::{StreamExt as _, TryStreamExt as _, future::BoxFuture, stream::BoxStream};
use sqlx::{
    Any, AnyConnection, Describe, Either, Error,
    any::{AnyArguments, AnyQueryResult, AnyRow, AnyStatement, AnyTypeInfo},
};
use sqlx_core::any::{
    AnyColumn, AnyConnectionBackend, AnyTypeInfoKind as Kind, AnyValue, AnyValueKind,
};
use std::{borrow::Cow, collections::VecDeque, sync::Arc};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
use tomato_core::{Connection, workbench::Cell};
type DbResult<T> = std::result::Result<T, Error>;
pub(super) fn database_name<'a>(connection: &'a Connection, database: &'a str) -> &'a str {
    let value = if database.is_empty() {
        connection.text("database")
    } else {
        database
    };
    tomato_core::database_scope::hive_name(value)
}
fn error(value: impl std::fmt::Display) -> Error {
    Error::Protocol(value.to_string())
}
fn failure(value: anyhow::Error) -> Error {
    if super::recovery::transport(&value) {
        Error::Io(std::io::Error::new(std::io::ErrorKind::NotConnected, value))
    } else {
        error(value)
    }
}
enum Event {
    Header(Vec<data::Column>),
    Row(AnyRow),
    Done(u64),
}
pub(super) struct Backend {
    client: Arc<Mutex<client::Client>>,
    cancel: CancellationToken,
}
impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiveSession")
            .field("closed", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub(super) async fn connect(
    connection: &Connection,
    database: &str,
    routed: Option<&Connection>,
) -> Result<AnyConnection> {
    let client =
        client::Client::connect(config::Config::new(connection, database, routed)?).await?;
    let cancel = client.cancel.clone();
    Ok(AnyConnection::from_backend(Backend {
        client: Arc::new(Mutex::new(client)),
        cancel,
    }))
}
fn arguments(arguments: AnyArguments<'_>) -> DbResult<Vec<Cell>> {
    arguments
        .values
        .0
        .into_iter()
        .map(|v| {
            Ok(match v {
                AnyValueKind::Null(_) => Cell::Null,
                AnyValueKind::Bool(v) => Cell::Boolean(v),
                AnyValueKind::SmallInt(v) => Cell::Integer(i64::from(v)),
                AnyValueKind::Integer(v) => Cell::Integer(i64::from(v)),
                AnyValueKind::BigInt(v) => Cell::Integer(v),
                AnyValueKind::Real(v) => Cell::Real(f64::from(v)),
                AnyValueKind::Double(v) => Cell::Real(v),
                AnyValueKind::Text(v) => Cell::Text(v.into_owned()),
                AnyValueKind::Blob(v) => Cell::Binary(v.into_owned()),
                _ => return Err(error("Hive 不支持此绑定类型")),
            })
        })
        .collect()
}
fn shape(columns: &[data::Column]) -> AnyRow {
    AnyRow {
        column_names: Arc::new(
            columns
                .iter()
                .enumerate()
                .map(|(i, c)| (c.name.clone().into(), i))
                .collect(),
        ),
        columns: columns
            .iter()
            .enumerate()
            .map(|(ordinal, c)| AnyColumn {
                ordinal,
                name: c.name.clone().into(),
                type_info: AnyTypeInfo {
                    kind: match c.data_type.as_str() {
                        "BOOLEAN" => Kind::Bool,
                        "TINYINT" | "SMALLINT" | "INT" | "BIGINT" => Kind::BigInt,
                        "FLOAT" | "DOUBLE" => Kind::Double,
                        "BINARY" => Kind::Blob,
                        _ => Kind::Text,
                    },
                },
            })
            .collect(),
        values: Vec::new(),
    }
}
fn row(values: Vec<Cell>, shape: &AnyRow) -> AnyRow {
    AnyRow {
        column_names: shape.column_names.clone(),
        columns: shape.columns.clone(),
        values: values
            .into_iter()
            .enumerate()
            .map(|(i, value)| AnyValue {
                kind: match value {
                    Cell::Null => AnyValueKind::Null(shape.columns[i].type_info.kind),
                    Cell::Boolean(v) => AnyValueKind::Bool(v),
                    Cell::Integer(v) => AnyValueKind::BigInt(v),
                    Cell::Real(v) => AnyValueKind::Double(v),
                    Cell::Text(v) => AnyValueKind::Text(Cow::Owned(v)),
                    Cell::Binary(v) => AnyValueKind::Blob(Cow::Owned(v)),
                },
            })
            .collect(),
    }
}
struct Cursor {
    client: Arc<Mutex<client::Client>>,
    guard: Option<OwnedMutexGuard<client::Client>>,
    sql: String,
    columns: Vec<data::Column>,
    shape: Option<AnyRow>,
    rows: VecDeque<Vec<Cell>>,
    stage: u8,
    affected: u64,
    count: usize,
    limit: Option<usize>,
}
impl Drop for Cursor {
    fn drop(&mut self) {
        if let Some(client) = &mut self.guard
            && client.operation.is_some()
        {
            client.disconnect();
        }
    }
}
impl Cursor {
    async fn next(&mut self) -> DbResult<Option<Event>> {
        if self.stage == 4 {
            return Ok(None);
        }
        if self.guard.is_none() {
            self.guard = Some(self.client.clone().lock_owned().await);
        }
        let client = self
            .guard
            .as_mut()
            .ok_or_else(|| error("Hive 会话不存在"))?;
        if self.stage == 0 {
            let (columns, affected) = client.start(&self.sql).await.map_err(failure)?;
            self.columns = columns;
            self.affected = affected;
            self.shape = Some(shape(&self.columns));
            self.stage = 1;
            return Ok(Some(Event::Header(self.columns.clone())));
        }
        if self.columns.is_empty() || self.limit.is_some_and(|limit| self.count >= limit) {
            client.close_operation().await.map_err(failure)?;
            self.stage = 4;
            return Ok(Some(Event::Done(self.affected)));
        }
        if self.rows.is_empty() {
            self.rows
                .extend(client.rows(&self.columns).await.map_err(failure)?);
            if self.rows.is_empty() {
                client.close_operation().await.map_err(failure)?;
                self.stage = 4;
                return Ok(Some(Event::Done(self.affected)));
            }
        }
        let values = self
            .rows
            .pop_front()
            .ok_or_else(|| error("Hive 结果行不存在"))?;
        self.count += 1;
        Ok(Some(Event::Row(row(
            values,
            self.shape
                .as_ref()
                .ok_or_else(|| error("Hive 结果列不存在"))?,
        ))))
    }
}
impl Backend {
    pub(super) async fn version(&mut self) -> Result<String> {
        self.client.lock().await.version().await
    }
    pub(super) async fn execute_raw(
        &self,
        sql: &str,
        row_limit: usize,
    ) -> Result<Vec<super::QueryResult>> {
        let mut output = Vec::new();
        let mut retained = 0usize;
        for sql in tomato_core::sql_script::split_for("HIVE", Some(sql)) {
            let mut result = super::QueryResult::default();
            // HS2 can close its operation explicitly. Stop after the first
            // omitted row instead of draining an unbounded result set.
            let mut events = self.events(sql, None, Some(row_limit.saturating_add(1)));
            while let Some(event) = events.try_next().await? {
                match event {
                    Event::Header(columns) => {
                        result.columns = columns.iter().map(|c| c.name.clone()).collect();
                        result.column_types = columns.iter().map(|c| c.jdbc).collect();
                        result.has_result_set = !columns.is_empty();
                        let metadata = result.retained_bytes();
                        anyhow::ensure!(
                            retained.saturating_add(metadata) <= tomato_core::limits::QUERY_BYTES,
                            "Hive 查询列信息超过 8 MiB"
                        );
                        retained += metadata;
                    }
                    Event::Row(row) => {
                        result.total_rows += 1;
                        if output.len() < tomato_core::limits::QUERY_RESULT_SETS
                            && result.rows.len() < row_limit
                            && retained < tomato_core::limits::QUERY_BYTES
                        {
                            let values = (0..result.columns.len())
                                .map(|index| super::cell(&row, index))
                                .collect::<Result<Vec<_>>>()?;
                            let bytes = result.row_bytes(&values);
                            if retained.saturating_add(bytes) <= tomato_core::limits::QUERY_BYTES {
                                retained += bytes;
                                result.push_row(values);
                            } else {
                                result.truncated = true;
                            }
                        } else {
                            result.truncated = true;
                        }
                    }
                    Event::Done(affected) => result.affected = affected,
                }
            }
            if output.len() < tomato_core::limits::QUERY_RESULT_SETS {
                output.push(result);
            } else if let Some(last) = output.last_mut() {
                last.truncated = true;
            }
        }
        Ok(output)
    }
    fn events(
        &self,
        sql: &str,
        args: Option<AnyArguments<'_>>,
        limit: Option<usize>,
    ) -> BoxStream<'static, DbResult<Event>> {
        let sql = args
            .map(arguments)
            .transpose()
            .and_then(|args| bind::apply(sql, args).map_err(error));
        let sql = match sql {
            Ok(sql) => sql,
            Err(error) => return Box::pin(futures_util::stream::once(async { Err(error) })),
        };
        let cursor = Cursor {
            client: self.client.clone(),
            guard: None,
            sql,
            columns: Vec::new(),
            shape: None,
            rows: VecDeque::new(),
            stage: 0,
            affected: 0,
            count: 0,
            limit,
        };
        Box::pin(futures_util::stream::try_unfold(
            cursor,
            |mut cursor| async move { Ok(cursor.next().await?.map(|event| (event, cursor))) },
        ))
    }
}
impl AnyConnectionBackend for Backend {
    fn name(&self) -> &str {
        "Hive"
    }
    fn close(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        Box::pin(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(300), async {
                self.client.lock().await.close().await
            })
            .await;
            self.cancel.cancel();
            Ok(())
        })
    }
    fn close_hard(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        Box::pin(async move {
            drop(self);
            Ok(())
        })
    }
    fn ping(&mut self) -> BoxFuture<'_, DbResult<()>> {
        Box::pin(async { self.client.lock().await.ping().await.map_err(failure) })
    }
    fn begin(&mut self, _: Option<Cow<'static, str>>) -> BoxFuture<'_, DbResult<()>> {
        Box::pin(async {
            Err(error("HiveServer2 使用语句自动提交，不提供客户端手动事务"))
        })
    }
    fn commit(&mut self) -> BoxFuture<'_, DbResult<()>> {
        Box::pin(async { Err(error("HiveServer2 不提供客户端手动提交")) })
    }
    fn rollback(&mut self) -> BoxFuture<'_, DbResult<()>> {
        Box::pin(async { Err(error("HiveServer2 不提供客户端手动回滚")) })
    }
    fn start_rollback(&mut self) {}
    fn get_transaction_depth(&self) -> usize {
        0
    }
    fn shrink_buffers(&mut self) {}
    fn flush(&mut self) -> BoxFuture<'_, DbResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn should_flush(&self) -> bool {
        false
    }
    fn fetch_many<'q>(
        &'q mut self,
        sql: &'q str,
        _: bool,
        arguments: Option<AnyArguments<'q>>,
    ) -> BoxStream<'q, DbResult<Either<AnyQueryResult, AnyRow>>> {
        Box::pin(self.events(sql, arguments, None).filter_map(|event| async {
            match event {
                Ok(Event::Header(_)) => None,
                Ok(Event::Row(row)) => Some(Ok(Either::Right(row))),
                Ok(Event::Done(rows_affected)) => Some(Ok(Either::Left(AnyQueryResult {
                    rows_affected,
                    last_insert_id: None,
                }))),
                Err(error) => Some(Err(error)),
            }
        }))
    }
    fn fetch_optional<'q>(
        &'q mut self,
        sql: &'q str,
        _: bool,
        arguments: Option<AnyArguments<'q>>,
    ) -> BoxFuture<'q, DbResult<Option<AnyRow>>> {
        Box::pin(async move {
            let mut events = self.events(sql, arguments, Some(1));
            let mut row = None;
            while let Some(event) = events.try_next().await? {
                if let Event::Row(value) = event {
                    row = Some(value);
                }
            }
            Ok(row)
        })
    }
    fn prepare_with<'c, 'q: 'c>(
        &'c mut self,
        _: &'q str,
        _: &[AnyTypeInfo],
    ) -> BoxFuture<'c, DbResult<AnyStatement<'q>>> {
        Box::pin(async {
            Err(error("HiveServer2 不提供独立预编译；参数在执行时安全绑定"))
        })
    }
    fn describe<'q>(&'q mut self, _: &'q str) -> BoxFuture<'q, DbResult<Describe<Any>>> {
        Box::pin(async {
            Err(error("Hive 查询列信息在执行后的服务器元数据中读取"))
        })
    }
}

pub(super) fn requires_password(connection: &Connection) -> bool {
    config::Config::new(connection, "", None).is_ok_and(|config| {
        config.sasl().unwrap_or(false) && !config.kerberos() && !config.username.is_empty()
    })
}
