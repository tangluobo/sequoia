//! Native Oracle transport behind the existing streaming SQLx session facade.
//! The blocking driver is confined to one owned worker; GUI futures never do I/O.
pub(super) mod catalog;
mod failure;
pub(super) mod identity;
pub(super) mod metadata;
#[cfg(test)]
mod tests;
mod values;
mod worker;
use super::QueryResult;
use failure::driver_error;
use futures_util::{StreamExt, TryStreamExt, future::BoxFuture, stream::BoxStream};
use sqlx::{
    Any, AnyConnection, Describe, Either, Error,
    any::{AnyArguments, AnyQueryResult, AnyRow, AnyStatement, AnyTypeInfo},
};
use sqlx_core::any::{AnyColumn, AnyConnectionBackend, AnyValueKind};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
};
use tokio::sync::{mpsc, oneshot};
use tomato_core::{Connection, limits};

type Result<T> = std::result::Result<T, Error>;
fn one_statement(sql: &str) -> Result<&str> {
    let mut statements = tomato_core::sql_script::split_for("ORACLE", Some(sql));
    let statement = statements.next().ok_or_else(|| error("Oracle SQL 为空"))?;
    if statements.next().is_some() {
        return Err(error(
            "参数化执行只能包含一条 Oracle 语句，请使用 SQL 脚本运行多条语句",
        ));
    }
    Ok(statement)
}
pub(super) struct Column {
    pub column: AnyColumn,
    pub nullable: bool,
    pub jdbc: i32,
}
pub(super) enum Event {
    Header(Vec<Column>),
    Row(AnyRow),
    Done(u64),
}
#[derive(Clone, Copy)]
enum Control {
    Ping,
    Begin,
    Commit,
    Rollback,
    Close,
}
struct Description {
    columns: Vec<AnyColumn>,
    nullable: Vec<Option<bool>>,
    parameters: usize,
}
enum Request {
    Query {
        sql: String,
        arguments: Vec<AnyValueKind<'static>>,
        limit: Option<usize>,
        fetch_size: u32,
        events: mpsc::Sender<Result<Event>>,
        cancelled: Arc<AtomicBool>,
    },
    Describe {
        sql: String,
        reply: oneshot::Sender<Result<Description>>,
    },
    Control {
        action: Control,
        reply: Option<oneshot::Sender<Result<()>>>,
    },
}
struct Cancel {
    flag: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    abort: Arc<oracledb::AbortHandle>,
    started: bool,
}
impl Cancel {
    fn complete(&mut self) {
        self.started = false;
    }
}
impl Drop for Cancel {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Release);
        if self.started {
            // Upstream has no concurrent cancel API. Interrupt blocking I/O and
            // invalidate the session rather than reuse an unread TTC response.
            self.dead.store(true, Ordering::Release);
            self.abort.abort();
        }
    }
}

pub(super) struct Backend {
    requests: mpsc::Sender<Request>,
    depth: Arc<AtomicUsize>,
    dead: Arc<AtomicBool>,
    abort: Arc<oracledb::AbortHandle>,
    fetch_size: Arc<AtomicU32>,
}
pub(super) struct FetchBudget {
    setting: Arc<AtomicU32>,
    previous: u32,
}
impl Drop for FetchBudget {
    fn drop(&mut self) {
        self.setting.store(self.previous, Ordering::Release);
    }
}
impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OracleSession")
            .field("transaction_depth", &self.get_transaction_depth())
            .finish_non_exhaustive()
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.dead.store(true, Ordering::Release);
        let _ = self.requests.try_send(Request::Control {
            action: Control::Close,
            reply: None,
        });
        self.abort.abort();
    }
}
fn error(value: impl std::fmt::Display) -> Error {
    Error::Protocol(value.to_string())
}
fn closed() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotConnected,
        "Oracle 会话已关闭或连接被中断，请重新连接",
    ))
}

pub(super) async fn connect(connection: &Connection) -> anyhow::Result<AnyConnection> {
    let descriptor = descriptor(connection)?;
    let config = oracledb::Config::default()
        .set_credentials(connection.text("username"), connection.text("password"))
        .set_stmtcachesize(8)
        .set_connect_string(&descriptor)
        .map_err(driver_error)?;
    let (requests, receive) = mpsc::channel(2);
    let (ready, opened) = oneshot::channel();
    let depth = Arc::new(AtomicUsize::new(0));
    let dead = Arc::new(AtomicBool::new(false));
    let worker_depth = depth.clone();
    let worker_dead = dead.clone();
    std::thread::Builder::new()
        .name("tomato-oracle".into())
        .spawn(move || {
            worker::run(config, receive, ready, worker_depth, worker_dead);
        })?;
    let abort = opened.await.map_err(|_| closed())??;
    Ok(AnyConnection::from_backend(Backend {
        requests,
        depth,
        dead,
        abort,
        fetch_size: Arc::new(AtomicU32::new(16)),
    }))
}

/// The legacy database field is the physical SID or /service, never an owner.
pub(super) fn descriptor(connection: &Connection) -> anyhow::Result<String> {
    let host = connection.text("host");
    let database = connection.text("database");
    let port = u16::try_from(connection.number("port"))?;
    let valid = |s: &str| {
        !s.is_empty()
            && !s
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '(' | ')' | '='))
            && s.trim() == s
    };
    anyhow::ensure!(valid(host) && port > 0, "Oracle 主机或端口无效");
    let (kind, database) = database
        .strip_prefix('/')
        .map_or(("SID", database), |s| ("SERVICE_NAME", s));
    anyhow::ensure!(
        valid(database),
        "请填写 Oracle SID；service name 请以 / 开头"
    );
    Ok(format!(
        "(DESCRIPTION=(TRANSPORT_CONNECT_TIMEOUT=10)(RETRY_COUNT=0)(ADDRESS=(PROTOCOL=TCP)(HOST={host})(PORT={port}))(CONNECT_DATA=({kind}={database})))"
    ))
}
impl Backend {
    pub(super) fn streaming(&self) -> FetchBudget {
        FetchBudget {
            setting: self.fetch_size.clone(),
            previous: self.fetch_size.swap(1, Ordering::AcqRel),
        }
    }
    async fn control(&self, action: Control) -> Result<()> {
        if self.dead.load(Ordering::Acquire) {
            return Err(closed());
        }
        let (reply, result) = oneshot::channel();
        self.requests
            .send(Request::Control {
                action,
                reply: Some(reply),
            })
            .await
            .map_err(|_| closed())?;
        result.await.map_err(|_| closed())?
    }
    async fn description(&self, sql: &str) -> Result<Description> {
        if self.dead.load(Ordering::Acquire) {
            return Err(closed());
        }
        let (reply, result) = oneshot::channel();
        self.requests
            .send(Request::Describe {
                sql: sql.into(),
                reply,
            })
            .await
            .map_err(|_| closed())?;
        result.await.map_err(|_| closed())?
    }
    fn events(
        &self,
        sql: &str,
        arguments: Option<AnyArguments<'_>>,
        limit: Option<usize>,
    ) -> BoxStream<'static, Result<Event>> {
        let sql = match one_statement(sql) {
            Ok(sql) => sql,
            Err(error) => return Box::pin(futures_util::stream::once(async { Err(error) })),
        };
        let owned = arguments.map_or_else(
            || Ok(Vec::new()),
            |args| {
                args.values
                    .0
                    .into_iter()
                    .map(|value| {
                        Ok(match value {
                            AnyValueKind::Null(v) => AnyValueKind::Null(v),
                            AnyValueKind::Bool(v) => AnyValueKind::Bool(v),
                            AnyValueKind::SmallInt(v) => AnyValueKind::SmallInt(v),
                            AnyValueKind::Integer(v) => AnyValueKind::Integer(v),
                            AnyValueKind::BigInt(v) => AnyValueKind::BigInt(v),
                            AnyValueKind::Real(v) => AnyValueKind::Real(v),
                            AnyValueKind::Double(v) => AnyValueKind::Double(v),
                            AnyValueKind::Text(v) => AnyValueKind::Text(Cow::Owned(v.into_owned())),
                            AnyValueKind::Blob(v) => AnyValueKind::Blob(Cow::Owned(v.into_owned())),
                            _ => return Err(error("Oracle 不支持此绑定类型")),
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            },
        );
        let owned = match owned {
            Ok(v) => v,
            Err(e) => return Box::pin(futures_util::stream::once(async { Err(e) })),
        };
        if self.dead.load(Ordering::Acquire) {
            return Box::pin(futures_util::stream::once(async { Err(closed()) }));
        }
        let (events, receive) = mpsc::channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = Request::Query {
            sql: sql.into(),
            arguments: owned,
            limit,
            fetch_size: self.fetch_size.load(Ordering::Acquire).clamp(1, 16),
            events,
            cancelled: cancelled.clone(),
        };
        let state = (
            self.requests.clone(),
            Some(request),
            receive,
            Cancel {
                flag: cancelled,
                dead: self.dead.clone(),
                abort: self.abort.clone(),
                started: false,
            },
            false,
        );
        Box::pin(futures_util::stream::unfold(
            state,
            |mut state| async move {
                if state.4 {
                    return None;
                }
                if let Some(request) = state.1.take() {
                    if state.0.send(request).await.is_err() {
                        state.4 = true;
                        return Some((Err(closed()), state));
                    }
                    state.3.started = true;
                }
                match state.2.recv().await {
                    Some(event) => {
                        if matches!(&event, Ok(Event::Done(_)) | Err(_)) {
                            state.3.complete();
                        }
                        Some((event, state))
                    }
                    None => {
                        state.3.complete();
                        None
                    }
                }
            },
        ))
    }
    pub(super) async fn raw(
        &mut self,
        sql: &str,
        row_limit: usize,
    ) -> anyhow::Result<Vec<QueryResult>> {
        let mut output = Vec::new();
        let mut bytes = 0usize;
        for statement in tomato_core::sql_script::split_for("ORACLE", Some(sql)) {
            let mut current = QueryResult::default();
            let mut events = self.events(statement, None, None);
            while let Some(event) = events.try_next().await? {
                match event {
                    Event::Header(columns) => {
                        current.has_result_set = true;
                        for c in columns {
                            use sqlx::Column as _;
                            bytes = bytes.saturating_add(c.column.name().len() + 32);
                            anyhow::ensure!(
                                bytes <= limits::QUERY_BYTES,
                                "Oracle 查询列信息超过 8 MiB"
                            );
                            current.columns.push(c.column.name().into());
                            current.column_types.push(c.jdbc);
                        }
                    }
                    Event::Row(row) => {
                        current.total_rows = current.total_rows.saturating_add(1);
                        if current.rows.len() < row_limit
                            && output.len() < limits::QUERY_RESULT_SETS
                            && bytes < limits::QUERY_BYTES
                        {
                            let values = (0..current.columns.len())
                                .map(|i| super::cell(&row, i))
                                .collect::<anyhow::Result<Vec<_>>>()?;
                            let held = current.row_bytes(&values);
                            if bytes.saturating_add(held) <= limits::QUERY_BYTES {
                                bytes += held;
                                current.push_row(values)
                            } else {
                                current.truncated = true;
                            }
                        } else {
                            current.truncated = true;
                        }
                    }
                    Event::Done(affected) => current.affected = affected,
                }
            }
            if output.len() < limits::QUERY_RESULT_SETS {
                output.push(current);
            } else if let Some(last) = output.last_mut() {
                last.truncated = true;
            }
        }
        Ok(output)
    }
}
impl AnyConnectionBackend for Backend {
    fn name(&self) -> &str {
        "Oracle"
    }
    fn close(self: Box<Self>) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move { self.control(Control::Close).await })
    }
    fn close_hard(self: Box<Self>) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            drop(self);
            Ok(())
        })
    }
    fn ping(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.control(Control::Ping))
    }
    fn begin(&mut self, statement: Option<Cow<'static, str>>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if statement
                .as_deref()
                .is_some_and(|s| !s.trim().eq_ignore_ascii_case("BEGIN"))
            {
                return Err(error("Oracle 不支持此自定义事务起始语句"));
            }
            self.control(Control::Begin).await
        })
    }
    fn commit(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.control(Control::Commit))
    }
    fn rollback(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.control(Control::Rollback))
    }
    fn start_rollback(&mut self) {
        if self
            .requests
            .try_send(Request::Control {
                action: Control::Rollback,
                reply: None,
            })
            .is_err()
        {
            self.dead.store(true, Ordering::Release);
            self.abort.abort();
        }
    }
    fn get_transaction_depth(&self) -> usize {
        self.depth.load(Ordering::Acquire)
    }
    fn shrink_buffers(&mut self) {}
    fn flush(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn should_flush(&self) -> bool {
        false
    }
    fn fetch_many<'q>(
        &'q mut self,
        sql: &'q str,
        _persistent: bool,
        arguments: Option<AnyArguments<'q>>,
    ) -> BoxStream<'q, Result<Either<AnyQueryResult, AnyRow>>> {
        Box::pin(self.events(sql, arguments, None).filter_map(|event| async {
            match event {
                Ok(Event::Header(_)) => None,
                Ok(Event::Row(row)) => Some(Ok(Either::Right(row))),
                Ok(Event::Done(rows_affected)) => Some(Ok(Either::Left(AnyQueryResult {
                    rows_affected,
                    last_insert_id: None,
                }))),
                Err(e) => Some(Err(e)),
            }
        }))
    }
    fn fetch_optional<'q>(
        &'q mut self,
        sql: &'q str,
        _persistent: bool,
        arguments: Option<AnyArguments<'q>>,
    ) -> BoxFuture<'q, Result<Option<AnyRow>>> {
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
        sql: &'q str,
        _parameters: &[AnyTypeInfo],
    ) -> BoxFuture<'c, Result<AnyStatement<'q>>> {
        Box::pin(async move {
            let info = self.description(sql).await?;
            Ok(AnyStatement {
                sql: Cow::Borrowed(sql),
                parameters: Some(Either::Right(info.parameters)),
                column_names: Arc::new(
                    info.columns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (c.name.clone(), i))
                        .collect(),
                ),
                columns: info.columns,
            })
        })
    }
    fn describe<'q>(&'q mut self, sql: &'q str) -> BoxFuture<'q, Result<Describe<Any>>> {
        Box::pin(async move {
            let info = self.description(sql).await?;
            Ok(Describe {
                columns: info.columns,
                parameters: Some(Either::Right(info.parameters)),
                nullable: info.nullable,
            })
        })
    }
}
