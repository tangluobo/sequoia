//! Generated DDL uses the same actor/session and preserves an existing transaction.
use super::*;
use sqlx::Executor;
use tomato_core::table_design::{Create, Dialect};

pub(in crate::database) async fn execute(
    session: &mut AnyConnection,
    kind: &str,
    request: Create,
) -> Result<TableId> {
    ensure!(
        matches!(kind, "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"),
        "当前数据库尚未实现创建表"
    );
    let backslash = if kind == "MYSQL" {
        let row = sqlx::query("SELECT @@SESSION.sql_mode")
            .fetch_one(&mut *session)
            .await?;
        !workbench::text(&row, 0)?
            .split(',')
            .any(|mode| mode.eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
    } else if matches!(kind, "ORACLE" | "HIVE") {
        false
    } else {
        let row = sqlx::query("SHOW standard_conforming_strings")
            .fetch_one(&mut *session)
            .await?;
        workbench::text(&row, 0)?.eq_ignore_ascii_case("off")
    };
    let statements = request.statements_for_session(Dialect::resolve(kind)?, backslash)?;
    let errors = request.validation_errors();
    ensure!(errors.is_empty(), "{}", errors.join("\n"));
    let point = workbench::savepoint_name(kind, "create");
    let transaction = workbench::existing_savepoint(session, kind, &point).await?;
    if transaction && matches!(kind, "MYSQL" | "ORACLE") {
        workbench::release_savepoint(session, kind, &point).await?;
        bail!("当前存在未完成事务，请先提交或回滚再创建表");
    }
    let own_transaction = kind == "POSTGRESQL" && !transaction;
    if own_transaction {
        session.execute("BEGIN").await?;
    }
    let result: Result<()> = async {
        for statement in &statements {
            session.execute(statement.as_str()).await?;
        }
        Ok(())
    }
    .await;
    if transaction {
        if result.is_err() {
            session
                .execute(format!("ROLLBACK TO SAVEPOINT {point}").as_str())
                .await?;
        }
        session
            .execute(format!("RELEASE SAVEPOINT {point}").as_str())
            .await?;
    } else if own_transaction {
        if result.is_ok() {
            if let Err(error) = session.execute("COMMIT").await {
                let _ = session.execute("ROLLBACK").await;
                return Err(error.into());
            }
        } else {
            session.execute("ROLLBACK").await?;
        }
    }
    if let Err(error) = result {
        if matches!(kind, "ORACLE" | "HIVE" | "MYSQL") {
            return Err(error.context("DDL 可能已部分提交，请刷新核对数据库实际结构"));
        }
        return Err(error);
    }
    Ok(request.table)
}
