//! Dedicated, bounded database sessions. Transfer never borrows query-tab transactions.
use super::*;
use crate::backup::Control;
use anyhow::{Context, ensure};
use sqlx::Executor;
use tomato_core::{
    table_design::{Dialect, alter::Edit},
    table_transfer::{Plan, Report, Request, TablePlan},
    workbench::{Cell, DatabaseAction, DatabaseOutput, TableId},
};
pub mod clipboard;
mod oracle;
mod postgres;
mod rows;
const BUDGET: usize = 8 * 1024 * 1024;
fn validate(request: &Request) -> Result<()> {
    ensure!(
        matches!(request.source.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE")
            && matches!(request.target.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE"),
        "此数据库的表传输适配器尚未迁移"
    );
    ensure!(
        !request.tables.is_empty() && request.tables.len() <= 1000,
        "请选择1至1,000张表"
    );
    ensure!(request.structure || request.data, "请选择复制结构或数据");
    ensure!(
        !((request.source.id == request.target.id
            || (request.source.kind() == request.target.kind()
                && request
                    .source
                    .text("host")
                    .eq_ignore_ascii_case(request.target.text("host"))
                && request.source.number("port") == request.target.number("port")))
            && request.source.text("database") == request.target.text("database")
            && request.source_schema == request.target_schema),
        "源表和目标表相同，请选择其他数据库或连接"
    );
    ensure!(
        request.tables.iter().map(String::len).sum::<usize>()
            + request.source_schema.len()
            + request.target_schema.len()
            < BUDGET,
        "传输请求超过8 MiB"
    );
    Ok(())
}
fn stamp(
    source: &tomato_core::table_design::alter::Snapshot,
    target: &Option<tomato_core::table_design::alter::Snapshot>,
) -> Result<String> {
    let bytes = serde_json::to_vec(&(source, target))?;
    ensure!(bytes.len() <= BUDGET, "表传输元数据超过8 MiB");
    Ok(ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
async fn plan(
    source: &mut Databases,
    target: &mut Databases,
    request: &Request,
    control: &Control,
    mut failures: Option<&mut Vec<(TableId, String)>>,
) -> Result<Plan> {
    validate(request)?;
    if request.source.kind() == request.target.kind()
        && (if request.source.kind() == "MYSQL" {
            request
                .source_schema
                .eq_ignore_ascii_case(&request.target_schema)
        } else {
            request.source_schema == request.target_schema
                && request.source.text("database") == request.target.text("database")
        })
    {
        let sql = if request.source.kind() == "MYSQL" {
            "SELECT @@server_uuid,CAST(@@lower_case_table_names AS CHAR)"
        } else if request.source.kind() == "ORACLE" {
            "SELECT SYS_CONTEXT('USERENV','DB_UNIQUE_NAME'),SYS_CONTEXT('USERENV','CON_NAME') FROM dual"
        } else {
            "SELECT pg_postmaster_start_time()::text,current_database()::text"
        };
        let a = workbench::metadata(source.session(&request.source).await?, sql, &[]).await?;
        let b = workbench::metadata(target.session(&request.target).await?, sql, &[]).await?;
        if let (Some(a), Some(b)) = (a.first(), b.first()) {
            let same_server = workbench::text(a, 0)? == workbench::text(b, 0)?
                && (request.source.kind() != "ORACLE"
                    || workbench::text(a, 1)? == workbench::text(b, 1)?);
            let same_schema = if request.source.kind() == "MYSQL" && workbench::text(a, 1)? != "0" {
                request
                    .source_schema
                    .eq_ignore_ascii_case(&request.target_schema)
            } else {
                request.source_schema == request.target_schema
            };
            ensure!(
                !(same_server && same_schema),
                "源连接和目标连接指向同一数据库中的同一张表，请选择其他数据库"
            );
        }
    }
    let DatabaseOutput::Tables(existing) = target
        .workbench(
            &request.target,
            DatabaseAction::Tables {
                schema: request.target_schema.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("目标表目录无效")
    };
    let mut output = Plan::default();
    let mut bytes = 0usize;
    for name in &request.tables {
        control.check()?;
        let src = TableId {
            schema: request.source_schema.clone(),
            name: name.clone(),
        };
        let dst = TableId {
            schema: request.target_schema.clone(),
            name: name.clone(),
        };
        let exists = existing.iter().any(|t| t.table == dst);
        if let Some(failures) = failures.as_deref_mut()
            && exists
        {
            failures.push((dst, format!("目标已存在同名对象：{name}")));
            continue;
        }
        let entry = match plan_table(source, target, request, &src, &dst, exists).await {
            Ok(entry) => entry,
            Err(error) => {
                let Some(failures) = failures.as_deref_mut() else {
                    return Err(error);
                };
                let mut message = format!("准备复制表 {name} 失败：{error:#}");
                message.truncate(message.floor_char_boundary(4096));
                failures.push((dst, message));
                continue;
            }
        };
        bytes = bytes.saturating_add(serde_json::to_vec(&entry)?.len());
        ensure!(bytes <= BUDGET, "传输计划超过8 MiB，请减少选择的表");
        output.tables.push(entry);
    }
    Ok(output)
}
async fn plan_table(
    source: &mut Databases,
    target: &mut Databases,
    request: &Request,
    src: &TableId,
    dst: &TableId,
    exists: bool,
) -> Result<TablePlan> {
    let name = &src.name;
    let original = objects::designer::read_for_transfer(
        source.session(&request.source).await?,
        request.source.kind(),
        src,
    )
    .await?;
    ensure!(!original.view, "不能通过表传输复制视图：{name}");
    let before = if exists {
        Some(
            objects::designer::read_for_transfer(
                target.session(&request.target).await?,
                request.target.kind(),
                dst,
            )
            .await?,
        )
    } else {
        None
    };
    ensure!(
        !before.as_ref().is_some_and(|s| s.view),
        "目标是视图：{name}"
    );
    let qualified = workbench::plan::qualified(request.target.kind(), dst)?;
    let mut sql = Vec::new();
    let mut data_sql = None;
    if request.drop_existing && (request.target.kind() != "ORACLE" || before.is_some()) {
        sql.push(format!(
            "DROP TABLE {}{qualified}",
            if request.target.kind() == "ORACLE" {
                ""
            } else {
                "IF EXISTS "
            }
        ));
    }
    if request.structure {
        if before.is_none() || request.drop_existing {
            // SHOW CREATE is retained verbatim after the quoted table header for MySQL.
            if request.source.kind() == "MYSQL" && request.target.kind() == "MYSQL" {
                let tokens = tomato_core::table_design::mysql_definition::tokens(
                    &original.sql,
                    original.backslash_escapes,
                )?;
                let body = tokens
                    .iter()
                    .find(|t| original.sql[t.range.clone()].starts_with('('))
                    .context("源CREATE TABLE缺少字段")?;
                sql.push(format!(
                    "CREATE TABLE {qualified} {}",
                    &original.sql[body.range.start..]
                ));
            } else if request.source.kind() == "ORACLE" && request.target.kind() == "ORACLE" {
                if oracle::same_connection(&request.source, &request.target) {
                    let original_table = workbench::plan::qualified("ORACLE", src)?;
                    sql.push(format!(
                        "CREATE TABLE {qualified} AS SELECT * FROM {original_table} WHERE 1=0"
                    ));
                    if request.data {
                        data_sql = Some(format!(
                            "INSERT INTO {qualified} SELECT * FROM {original_table}"
                        ));
                    }
                } else {
                    sql.push(
                        oracle::create(source.session(&request.source).await?, src, dst).await?,
                    );
                }
            } else if request.source.kind() == "POSTGRESQL"
                && request.target.kind() == "POSTGRESQL"
                && request.source.id == request.target.id
                && request.source.text("database") == request.target.text("database")
            {
                sql.push(format!(
                    "CREATE TABLE {qualified} (LIKE {} INCLUDING ALL)",
                    workbench::plan::qualified("POSTGRESQL", src)?
                ));
            } else if request.source.kind() == "POSTGRESQL" && request.target.kind() == "POSTGRESQL"
            {
                sql.extend(
                    postgres::create(source.session(&request.source).await?, src, dst).await?,
                );
            } else {
                let mut definition = original.definition.clone();
                definition.table = dst.clone();
                adapt(
                    &mut definition,
                    request.source.kind(),
                    request.target.kind(),
                )?;
                sql.extend(definition.statements_for_session(
                    Dialect::resolve(request.target.kind())?,
                    before.as_ref().is_some_and(|s| s.backslash_escapes),
                )?);
            }
        } else if let Some(before) = &before {
            let mut after = before.definition.clone();
            let mut desired = original.definition.clone();
            adapt(&mut desired, request.source.kind(), request.target.kind())?;
            after.columns = desired
                .columns
                .iter()
                .filter_map(|column| {
                    let old = before
                        .definition
                        .columns
                        .iter()
                        .find(|c| c.name == column.name);
                    if old.is_none() && !request.add {
                        None
                    } else {
                        Some(match old {
                            Some(old) if !request.modify => old.clone(),
                            _ => column.clone(),
                        })
                    }
                })
                .collect();
            if !request.drop {
                after.columns.extend(
                    before
                        .definition
                        .columns
                        .iter()
                        .filter(|c| !desired.columns.iter().any(|s| s.name == c.name))
                        .cloned(),
                );
            }
            let origins = after
                .columns
                .iter()
                .map(|c| {
                    before
                        .definition
                        .columns
                        .iter()
                        .any(|b| b.name == c.name)
                        .then(|| c.name.clone())
                })
                .collect();
            sql.extend(
                Edit {
                    before: before.baseline(),
                    after,
                    origins,
                }
                .plan(
                    Dialect::resolve(request.target.kind())?,
                    before.backslash_escapes,
                )?
                .statements,
            );
        }
    }
    // Original cross-connection execution syncs only structure for an existing table.
    let data = request.data
        && (before.is_none()
            || request.drop_existing
            || !request.structure
            || request.source.id == request.target.id);
    ensure!(
        request.structure || before.is_some(),
        "只复制数据时目标表必须存在：{name}"
    );
    Ok(TablePlan {
        source: src.clone(),
        target: dst.clone(),
        sql,
        data,
        data_sql,
        stamp: stamp(&original, &before)?,
    })
}
fn adapt(
    definition: &mut tomato_core::table_design::Create,
    source: &str,
    target: &str,
) -> Result<()> {
    if source == target {
        return Ok(());
    }
    definition.options = Default::default();
    for column in &mut definition.columns {
        let kind = column.data_type.to_ascii_lowercase();
        if source == "ORACLE" || target == "ORACLE" {
            oracle::adapt(column, source, target)?;
            continue;
        }
        column.data_type = match (source, target, kind.as_str()) {
            ("MYSQL", "POSTGRESQL", "tinyint" | "smallint") => "smallint".into(),
            ("MYSQL", "POSTGRESQL", "int" | "integer" | "mediumint") => "integer".into(),
            ("MYSQL", "POSTGRESQL", "datetime") => "timestamp".into(),
            ("MYSQL", "POSTGRESQL", "double") => "double precision".into(),
            (
                "MYSQL",
                "POSTGRESQL",
                "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary",
            ) => "bytea".into(),
            ("MYSQL", "POSTGRESQL", "tinytext" | "mediumtext" | "longtext") => "text".into(),
            ("POSTGRESQL", "MYSQL", "int2" | "smallint") => "SMALLINT".into(),
            ("POSTGRESQL", "MYSQL", "int4" | "integer") => "INT".into(),
            ("POSTGRESQL", "MYSQL", "int8" | "bigint") => "BIGINT".into(),
            ("POSTGRESQL", "MYSQL", "boolean" | "bool") => "BOOLEAN".into(),
            ("POSTGRESQL", "MYSQL", "bytea") => "LONGBLOB".into(),
            ("POSTGRESQL", "MYSQL", "double precision" | "float8") => "DOUBLE".into(),
            ("POSTGRESQL", "MYSQL", "timestamp without time zone") => "DATETIME".into(),
            (
                _,
                _,
                "varchar" | "char" | "text" | "bigint" | "decimal" | "numeric" | "date" | "time"
                | "timestamp" | "json" | "real" | "float",
            ) => kind,
            _ => anyhow::bail!(
                "字段 {} 的类型 {} 无法无损映射到目标数据库",
                column.name,
                column.data_type
            ),
        };
        if !matches!(
            column.data_type.as_str(),
            "varchar" | "char" | "decimal" | "numeric"
        ) {
            column.length.clear();
            column.scale.clear();
        }
        column.charset.clear();
        column.collation.clear();
        column.unsigned = false;
        column.zerofill = false;
        if source == "POSTGRESQL" && column.auto_increment {
            column.default_value.clear();
        }
    }
    Ok(())
}
pub async fn preview(request: &Request, control: &Control) -> Result<Plan> {
    let mut source = Databases::default();
    let mut target = Databases::default();
    let result = tokio::select! {
        biased;
        () = control.wait_cancelled() => anyhow::bail!("操作已取消"),
        result = plan(&mut source, &mut target, request, control, None) => result,
    };
    let a = source.disconnect(&request.source.id).await;
    let b = target.disconnect(&request.target.id).await;
    let result = result?;
    a?;
    b?;
    Ok(result)
}
pub async fn execute(request: &Request, accepted: &Plan, control: &Control) -> Result<Report> {
    run(request, Some(accepted), control).await
}
pub async fn paste(request: &Request, control: &Control) -> Result<Report> {
    ensure!(
        request.source.id == request.target.id
            && request.source.kind() == request.target.kind()
            && request.structure
            && request.data
            && !request.drop_existing,
        "直接粘贴仅支持同一连接中新建表并复制数据"
    );
    run(request, None, control).await
}
async fn run(request: &Request, accepted: Option<&Plan>, control: &Control) -> Result<Report> {
    let mut source = Databases::default();
    let mut target = Databases::default();
    let mut failures = Vec::new();
    // Preview and revalidation own their sessions and perform no writes.
    // Dropping them on cancellation releases a pending metadata read.
    let current = tokio::select! {
        biased;
        () = control.wait_cancelled() => anyhow::bail!("操作已取消"),
        result = plan(&mut source, &mut target, request, control, accepted.is_none().then_some(&mut failures)) => result,
    };
    let result = async {
        let current = current?;
        ensure!(
            accepted.is_none_or(|plan| &current == plan),
            "源表或目标表结构已变化，请重新生成SQL预览后执行"
        );
        control.reset((current.tables.len() + failures.len()) as u64);
        let mut report = Report {
            failed: failures,
            ..Report::default()
        };
        for (_, error) in &report.failed {
            control.log(error);
            control.object();
        }
        for table in current.tables {
            if control.cancelled() {
                report.cancelled = true;
                break;
            }
            let result = async {
                for sql in &table.sql {
                    control.check()?;
                    control.log(sql);
                    target
                        .session(&request.target)
                        .await?
                        .execute(sql.as_str())
                        .await?;
                }
                if let Some(sql) = &table.data_sql {
                    control.check()?;
                    control.log(sql);
                    oracle::copy_in_database(target.session(&request.target).await?, sql, control)
                        .await
                } else if table.data {
                    rows::copy(
                        source.session(&request.source).await?,
                        target.session(&request.target).await?,
                        request,
                        &table,
                        control,
                    )
                    .await
                } else {
                    Ok(0)
                }
            }
            .await;
            match result {
                Ok(records) => {
                    report.records = report.records.saturating_add(records);
                    report.succeeded.push(table.target);
                    control.log("SUCCESS 复制完成");
                }
                Err(error) => {
                    control.log(&format!("ERROR {}: {error}", table.target.name));
                    report.failed.push((table.target, error.to_string()));
                }
            }
            control.object();
            if control.cancelled() {
                report.cancelled = true;
                break;
            }
        }
        report.records = control.read(|p| p.records).unwrap_or(report.records);
        Ok(report)
    }
    .await;
    let a = source.disconnect(&request.source.id).await;
    let b = target.disconnect(&request.target.id).await;
    let result = result?;
    a?;
    b?;
    Ok(result)
}
