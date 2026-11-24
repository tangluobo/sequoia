use super::*;
use oracledb::ToDbValue;
use std::{collections::HashSet, sync::atomic::Ordering, time::Duration};

pub(super) fn run(
    config: oracledb::Config,
    mut requests: mpsc::Receiver<Request>,
    ready: oneshot::Sender<Result<Arc<oracledb::AbortHandle>>>,
    depth: Arc<AtomicUsize>,
    dead: Arc<AtomicBool>,
) {
    let opened = (|| {
        let connection = oracledb::connect(config).map_err(driver_error)?;
        connection
            .set_call_timeout(Some(Duration::from_secs(30)))
            .map_err(driver_error)?;
        connection
            .execute("ALTER SESSION SET NLS_NUMERIC_CHARACTERS='.,'", &[])
            .map_err(driver_error)?;
        // Java's Oracle JDBC route has no blanket query timeout. The bounded
        // initialization above must not abort long DDL, LOB or restore jobs;
        // their owning future can still interrupt the socket through AbortHandle.
        connection.set_call_timeout(None).map_err(driver_error)?;
        let abort = Arc::new(connection.abort_handle().map_err(driver_error)?);
        Ok::<_, Error>((connection, abort))
    })();
    let (mut connection, abort) = match opened {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    if ready.send(Ok(abort.clone())).is_err() {
        return;
    }
    while !dead.load(Ordering::Acquire) {
        let Some(request) = requests.blocking_recv() else {
            break;
        };
        match request {
            Request::Query {
                sql,
                arguments,
                limit,
                fetch_size,
                events,
                cancelled,
            } => {
                if let Err(e) = execute(
                    &connection,
                    &sql,
                    arguments,
                    (limit, fetch_size),
                    &events,
                    &cancelled,
                    &depth,
                ) {
                    stop_on_transport_error(&e, &dead, &abort);
                    let _ = events.blocking_send(Err(e));
                }
            }
            Request::Describe { sql, reply } => {
                let result = (|| {
                    let mut statement = connection
                        .statement(&sql)
                        .map_err(driver_error)?
                        .fetch_lobs()
                        .prefetch_rows(0)
                        .fetch_array_size(16)
                        .build()
                        .map_err(driver_error)?;
                    // Oracle executes DDL even in parse-only mode. Never describe DDL by executing it.
                    if statement.is_query() {
                        statement.ensure_fully_parsed().map_err(driver_error)?;
                    }
                    let mut seen = HashSet::new();
                    let parameters = statement
                        .bind_names()
                        .iter()
                        .filter(|n| seen.insert(n.as_str()))
                        .count();
                    let columns = values::columns(statement.out_metadata())?;
                    Ok(Description {
                        nullable: columns.iter().map(|c| Some(c.nullable)).collect(),
                        columns: columns.into_iter().map(|c| c.column).collect(),
                        parameters,
                    })
                })();
                if let Err(error) = &result {
                    stop_on_transport_error(error, &dead, &abort);
                }
                let _ = reply.send(result);
            }
            Request::Control { action, reply } => {
                if matches!(action, Control::Close) {
                    // Reply only after LOGOFF/EOF. An earlier acknowledgement
                    // lets Backend::drop abort the socket during graceful close.
                    let result = connection
                        .set_call_timeout(Some(Duration::from_secs(5)))
                        .and_then(|()| connection.close())
                        .map_err(driver_error);
                    dead.store(true, Ordering::Release);
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                    return;
                }
                let result = control(&connection, action, &depth);
                if let Err(error) = &result {
                    stop_on_transport_error(error, &dead, &abort);
                }
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
        }
    }
    let _ = connection.close();
    dead.store(true, Ordering::Release);
}
fn stop_on_transport_error(error: &Error, dead: &AtomicBool, abort: &oracledb::AbortHandle) {
    if let Error::Io(error) = error
        && matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::TimedOut
        )
    {
        dead.store(true, Ordering::Release);
        abort.abort();
    }
}
fn control(connection: &oracledb::Connection, action: Control, depth: &AtomicUsize) -> Result<()> {
    let current = depth.load(Ordering::Acquire);
    match action {
        Control::Ping => connection.ping().map_err(driver_error)?,
        Control::Begin => {
            if current > 0 {
                connection
                    .execute(&format!("SAVEPOINT TOMATO_SP_{current}"), &[])
                    .map_err(driver_error)?;
            }
            depth.store(current + 1, Ordering::Release);
        }
        Control::Commit => {
            if current <= 1 {
                connection.commit().map_err(driver_error)?;
            }
            depth.store(current.saturating_sub(1), Ordering::Release);
        }
        Control::Rollback => {
            if current > 1 {
                connection
                    .execute(
                        &format!("ROLLBACK TO SAVEPOINT TOMATO_SP_{}", current - 1),
                        &[],
                    )
                    .map_err(driver_error)?;
            } else {
                connection.rollback().map_err(driver_error)?;
            }
            depth.store(current.saturating_sub(1), Ordering::Release);
        }
        Control::Close => {}
    }
    Ok(())
}
fn arguments(values: Vec<AnyValueKind<'static>>) -> Result<Vec<Box<dyn ToDbValue>>> {
    values
        .into_iter()
        .map(|v| {
            Ok(match v {
                AnyValueKind::Null(_) => Box::new(Option::<String>::None) as Box<dyn ToDbValue>,
                AnyValueKind::Bool(v) => Box::new(v),
                AnyValueKind::SmallInt(v) => Box::new(v),
                AnyValueKind::Integer(v) => Box::new(v),
                AnyValueKind::BigInt(v) => Box::new(v),
                AnyValueKind::Real(v) => Box::new(v),
                AnyValueKind::Double(v) => Box::new(v),
                AnyValueKind::Text(v) => Box::new(v.into_owned()),
                AnyValueKind::Blob(v) => Box::new(v.into_owned()),
                _ => return Err(error("Oracle 不支持此绑定值")),
            })
        })
        .collect()
}
fn execute(
    connection: &oracledb::Connection,
    sql: &str,
    arguments_value: Vec<AnyValueKind<'static>>,
    bounds: (Option<usize>, u32),
    events: &mpsc::Sender<Result<Event>>,
    cancelled: &AtomicBool,
    depth: &AtomicUsize,
) -> Result<()> {
    let (limit, fetch_size) = bounds;
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    // The Oracle server requires END; on PL/SQL units. SQL statement terminators
    // have already been removed by the dialect-aware borrowed lexer.
    let text = sql.trim();
    let action =
        if text.eq_ignore_ascii_case("BEGIN") || text.eq_ignore_ascii_case("START TRANSACTION") {
            Some(Control::Begin)
        } else if text.eq_ignore_ascii_case("COMMIT") {
            Some(Control::Commit)
        } else if text.eq_ignore_ascii_case("ROLLBACK") {
            Some(Control::Rollback)
        } else {
            None
        };
    if let Some(action) = action {
        control(connection, action, depth)?;
        let _ = events.blocking_send(Ok(Event::Done(0)));
        return Ok(());
    }
    let mut statement = connection
        .statement(text)
        .map_err(driver_error)?
        .autocommit(
            depth.load(Ordering::Acquire) == 0
                && !text.to_ascii_uppercase().starts_with("ALTER SESSION"),
        )
        .fetch_lobs()
        .prefetch_rows(fetch_size)
        .fetch_array_size(fetch_size)
        .build()
        .map_err(driver_error)?;
    let arguments = arguments(arguments_value)?;
    let mut seen = HashSet::new();
    let names = statement
        .bind_names()
        .iter()
        .filter(|n| seen.insert(n.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if names.len() != arguments.len() {
        return Err(error(format!(
            "Oracle 绑定数不匹配：需要 {}，实际 {}",
            names.len(),
            arguments.len()
        )));
    }
    let binds = names
        .iter()
        .zip(&arguments)
        .map(|(name, value)| (name.as_str(), value.as_ref()))
        .collect::<Vec<_>>();
    if statement.is_query() {
        let mut cursor = statement.query_named(&binds).map_err(driver_error)?;
        let columns = values::columns(cursor.columns())?;
        let shape = values::Shape::new(&columns);
        if events.blocking_send(Ok(Event::Header(columns))).is_err() {
            return Ok(());
        }
        let mut count = 0usize;
        while !cancelled.load(Ordering::Acquire) && limit.is_none_or(|limit| count < limit) {
            let Some(row) = cursor.next() else {
                break;
            };
            let row = values::row(row.map_err(driver_error)?, &shape)?;
            count += 1;
            if events.blocking_send(Ok(Event::Row(row))).is_err() {
                return Ok(());
            }
        }
        let _ = events.blocking_send(Ok(Event::Done(0)));
    } else {
        let result = statement.execute_named(&binds).map_err(driver_error)?;
        let _ = events.blocking_send(Ok(Event::Done(result.rows_affected())));
    }
    Ok(())
}
