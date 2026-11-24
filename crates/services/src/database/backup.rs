//! Database actor orchestration. PostgreSQL jobs own a cancellable session on the existing route.
use super::*;
use crate::backup::{Control, Repository};
use anyhow::Context;
use futures_util::FutureExt as _;
use sqlx::Executor;
use std::path::PathBuf;
use tomato_core::backup::{Operation, Report};
mod catalog;
mod mysql;
mod oracle;
mod oracle_values;
mod postgres;
mod restore;
#[derive(Clone, Copy)]
enum Mode {
    None,
    Snapshot,
    Locked,
}
async fn begin(
    conn: &mut AnyConnection,
    kind: &str,
    request: &tomato_core::backup::Create,
) -> Result<Mode> {
    if kind == "ORACLE" {
        conn.execute("BEGIN").await?;
        let outcome: Result<()> = async {
            conn.execute(if request.lock_tables {
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"
            } else {
                "SET TRANSACTION READ ONLY"
            })
            .await?;
            if request.lock_tables {
                for object in request
                    .objects
                    .iter()
                    .filter(|object| object.kind == tomato_core::backup::Kind::Table)
                {
                    let table = tomato_core::workbench::TableId {
                        schema: if object.table.schema.is_empty() {
                            request.scope.database.clone()
                        } else {
                            object.table.schema.clone()
                        },
                        name: object.table.name.clone(),
                    };
                    conn.execute(
                        format!(
                            "LOCK TABLE {} IN SHARE MODE",
                            workbench::plan::qualified("ORACLE", &table)?
                        )
                        .as_str(),
                    )
                    .await?;
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            let _ = conn.execute("ROLLBACK").await;
            return Err(error);
        }
        return Ok(Mode::Snapshot);
    }
    if kind == "POSTGRESQL" {
        conn.execute(if request.lock_tables {
            "BEGIN ISOLATION LEVEL REPEATABLE READ"
        } else {
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY"
        })
        .await?;
        let result: Result<()> = async {
            conn.execute("SET LOCAL search_path=pg_catalog; SET LOCAL row_security=off; SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='3s'; SET LOCAL DateStyle='ISO'; SET LOCAL IntervalStyle='postgres'; SET LOCAL TimeZone='UTC'; SET LOCAL bytea_output='hex'; SET LOCAL extra_float_digits=3").await?;
            if request.lock_tables {
                let names=request.objects.iter().filter(|object| object.kind==tomato_core::backup::Kind::Table).map(|object|postgres::qualified(&object.table)).collect::<Result<Vec<_>>>()?;
                if !names.is_empty() {conn.execute(format!("LOCK TABLE {} IN SHARE MODE",names.join(",")).as_str()).await?;}
            }
            Ok(())
        }.await;
        if let Err(error) = result {
            conn.execute("ROLLBACK").await?;
            return Err(error);
        }
        return Ok(Mode::Snapshot);
    }
    if request.lock_tables {
        let tables = workbench::metadata(
            conn,
            "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA=?",
            &[tomato_core::workbench::Cell::Text(
                request.scope.database.clone(),
            )],
        )
        .await?;
        if tables.is_empty() {
            return Ok(Mode::None);
        }
        let dialect = crate::dialect::resolve("MYSQL")?;
        let names = tables
            .iter()
            .map(|row| {
                Ok(format!(
                    "{}.{} READ",
                    dialect.quote(&request.scope.database)?,
                    dialect.quote(&workbench::text(row, 0)?)?
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .join(",");
        conn.execute(format!("LOCK TABLES {names}").as_str())
            .await?;
        Ok(Mode::Locked)
    } else if request.single_transaction {
        conn.execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .await?;
        conn.execute("START TRANSACTION WITH CONSISTENT SNAPSHOT")
            .await?;
        Ok(Mode::Snapshot)
    } else {
        Ok(Mode::None)
    }
}
async fn finish(conn: &mut AnyConnection, mode: Mode) -> Result<()> {
    match mode {
        Mode::None => {}
        Mode::Snapshot => {
            conn.execute("ROLLBACK").await?;
        }
        Mode::Locked => {
            conn.execute("UNLOCK TABLES").await?;
        }
    }
    Ok(())
}
impl Databases {
    pub async fn backup(
        &mut self,
        connection: &Connection,
        root: PathBuf,
        operation: Operation,
        control: Option<Control>,
    ) -> Result<Report> {
        anyhow::ensure!(
            operation.retained_bytes() <= 8 * 1024 * 1024,
            "备份请求超过 8 MiB"
        );
        anyhow::ensure!(
            matches!(connection.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE"),
            "此数据库的备份与还原适配器尚未迁移"
        );
        let scope = match &operation {
            Operation::Create(request) => &request.scope,
            Operation::Restore(request) => &request.scope,
        };
        anyhow::ensure!(
            scope.connection == connection.name,
            "备份目录与连接名称不一致"
        );
        let target = tomato_core::database_scope::select(connection, &scope.database);
        let control = control.unwrap_or_default();
        control.check()?;
        let conn = self.session(&target).await?;
        let savepoint = format!("tomato_backup_{}", uuid::Uuid::new_v4().simple());
        if workbench::existing_savepoint(conn, connection.kind(), &savepoint).await? {
            workbench::release_savepoint(conn, connection.kind(), &savepoint).await?;
            anyhow::bail!("当前连接存在未完成的手动事务，请先提交或回滚");
        }
        let committing = std::sync::atomic::AtomicBool::new(false);
        if connection.kind() == "ORACLE" {
            let routed = self
                .tunnels
                .get(&target.id)
                .map(|tunnel| tunnel.routed(&target))
                .unwrap_or_else(|| target.clone());
            let mut owned = tokio::select! {result=super::oracle::connect(&routed)=>result?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
            let outcome = {
                let operation = execute(
                    &mut owned,
                    connection,
                    root,
                    operation,
                    &control,
                    &committing,
                );
                tokio::pin!(operation);
                tokio::select! {result=&mut operation=>Some(result),()=control.wait_cancelled()=>{if committing.load(std::sync::atomic::Ordering::Acquire){Some(operation.await)}else{None}}}
            };
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), owned.close()).await;
            return outcome.unwrap_or_else(|| {
                Err(anyhow::anyhow!(
                    "操作已取消，Oracle 备份会话已释放；DDL 或先前批次可能已提交，请核对数据库"
                ))
            });
        }
        if connection.kind() != "POSTGRESQL" {
            return execute(conn, connection, root, operation, &control, &committing).await;
        }
        // The caller's session settings and transactions never belong to a cancellable dump.
        let routed = self
            .tunnels
            .get(&target.id)
            .map(|tunnel| tunnel.routed(&target))
            .unwrap_or_else(|| target.clone());
        let uri = super::connection_uri(&routed)?;
        let mut owned = tokio::select! {
            result=tokio::time::timeout(std::time::Duration::from_secs(10),AnyConnection::connect(&uri))=>result??,
            ()=control.wait_cancelled()=>anyhow::bail!("操作已取消"),
        };
        let tag = format!("tomato-backup-{}", uuid::Uuid::new_v4().simple());
        tokio::select! {
            result=sqlx::query("SELECT pg_catalog.set_config('application_name',$1,false)").bind(&tag).execute(&mut owned)=>{result?;},
            ()=control.wait_cancelled()=>anyhow::bail!("操作已取消"),
        }
        let outcome = {
            let operation = execute(
                &mut owned,
                connection,
                root,
                operation,
                &control,
                &committing,
            );
            tokio::pin!(operation);
            tokio::select! {
                result=&mut operation=>Some(result),
                ()=control.wait_cancelled()=>{
                    if committing.load(std::sync::atomic::Ordering::Acquire){Some(operation.await)}else{None}
                }
            }
        };
        if outcome.is_none() {
            // A unique tag plus database and login scope cannot signal an unrelated session.
            let caller = self.session(&target).await?;
            let cancel=sqlx::query("SELECT pg_catalog.pg_cancel_backend(pid) FROM pg_catalog.pg_stat_activity WHERE application_name=$1 AND datname=pg_catalog.current_database() AND usename=session_user AND pid<>pg_catalog.pg_backend_pid()")
                .bind(&tag).execute(caller);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), cancel).await;
        }
        // Closing this job's socket releases its transaction even if signaling is unavailable.
        // A commit already sent is awaited above, so a committed restore cannot report cancellation.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), owned.close()).await;
        outcome.unwrap_or_else(|| Err(anyhow::anyhow!("操作已取消，备份会话已释放")))
    }
}
async fn execute(
    conn: &mut AnyConnection,
    connection: &Connection,
    root: PathBuf,
    operation: Operation,
    control: &Control,
    committing: &std::sync::atomic::AtomicBool,
) -> Result<Report> {
    let started = std::time::Instant::now();
    let (path, objects, records) = match operation {
        Operation::Create(request) => {
            anyhow::ensure!(
                !request.objects.is_empty() && request.objects.len() <= 10_000,
                "请选择 1 至 10,000 个备份对象"
            );
            let path = Repository::new(root).create_path(&request.scope, &request.filename)?;
            let directory = path.parent().context("备份目录无效")?;
            let mode = begin(conn, connection.kind(), &request).await?;
            let prepared = if connection.kind() == "POSTGRESQL" {
                postgres::objects(conn, &request, directory, control)
                    .boxed()
                    .await
            } else if connection.kind() == "ORACLE" {
                oracle::objects(conn, &request, directory, control)
                    .boxed()
                    .await
            } else {
                mysql::objects(conn, connection, &request, directory, control)
                    .boxed()
                    .await
            };
            let cleanup = finish(conn, mode).await;
            let (writer, metadata, records) = prepared?;
            cleanup?;
            control.check()?;
            let objects = metadata.objects.len() as u64;
            writer.finish(&path, &metadata)?;
            (path, objects, records)
        }
        Operation::Restore(request) => {
            if connection.kind() == "POSTGRESQL" {
                postgres::restore(conn, &root, &request, control, committing)
                    .boxed()
                    .await?
            } else if connection.kind() == "ORACLE" {
                oracle::restore(conn, &root, &request, control, committing)
                    .boxed()
                    .await?
            } else {
                restore::mysql(conn, connection, &root, &request, control).await?
            }
        }
    };
    Ok(Report {
        path,
        objects,
        records,
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}
