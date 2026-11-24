//! Dedicated sessions, typed parameters and table/batch transactions for file imports.
mod convert;
#[cfg(test)]
mod tests;
use super::{Databases, workbench};
use crate::data_import::{Control, Prepared};
use anyhow::{Context, Result, ensure};
pub use convert::column_type;
use sqlx::{AnyConnection, Connection as _, Executor};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    sync::Arc,
};
use tomato_core::{
    Connection,
    data_import::*,
    workbench::{
        Cell, Column, DatabaseAction, DatabaseOutput, RowChange, TableDefinition, TableId,
        TableInfo,
    },
};
pub struct Request {
    pub connection: Connection,
    pub database: String,
    pub prepared: Arc<Prepared>,
    pub targets: Vec<Target>,
    pub options: Options,
}
pub async fn tables(
    connection: &Connection,
    database: &str,
    schema: &str,
    control: &Control,
) -> Result<Vec<TableInfo>> {
    let mut db = Databases::default();
    let scoped = tomato_core::database_scope::select(connection, database);
    let out = tokio::select! {out=db.workbench(&scoped,DatabaseAction::Tables{schema:schema.into()})=>out?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    let DatabaseOutput::Tables(tables) = out else {
        anyhow::bail!("目标对象列表无效")
    };
    Ok(tables.into_iter().filter(|t| !t.view).collect())
}
pub async fn definition(
    connection: &Connection,
    database: &str,
    table: &TableId,
    control: &Control,
) -> Result<TableDefinition> {
    let mut db = Databases::default();
    let conn = tokio::select! {r=db.session_in(connection,database)=>r?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    tokio::select! {r=workbench::describe(conn,connection.kind(),table)=>r,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")}
}
pub fn defaults(
    source: usize,
    table: &SourceTable,
    target: TableId,
    existing: Option<TableDefinition>,
) -> Target {
    let fields = table
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let other = existing.as_ref().and_then(|d| {
                d.columns
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(&f.name))
            });
            Mapping {
                enabled: existing.is_none() || other.is_some_and(|c| !c.generated),
                source: Some(i),
                source_name: Some(f.name.clone()),
                default: None,
                target: other.map_or_else(|| f.name.clone(), |c| c.name.clone()),
                data_type: other.map_or_else(|| f.data_type.clone(), column_type),
                nullable: other.map_or(f.nullable, |c| c.nullable),
                key: other.is_some_and(|c| c.primary_key > 0),
            }
        })
        .collect();
    Target {
        selected: true,
        source,
        source_key: Some((table.origin.clone(), table.name.clone())),
        table: target,
        create: existing.is_none(),
        fields,
        expected: existing,
    }
}
pub fn validate(request: &Request) -> Result<()> {
    ensure!(
        matches!(
            request.connection.kind(),
            "MYSQL" | "POSTGRESQL" | "SQLITE" | "ORACLE"
        ),
        "当前数据库不支持数据导入"
    );
    ensure!(
        (1..=10000).contains(&request.options.batch_size),
        "每批记录数应为 1–10000"
    );
    let mut destinations = std::collections::HashSet::new();
    let selected = request
        .targets
        .iter()
        .filter(|t| t.selected)
        .collect::<Vec<_>>();
    ensure!(
        !selected.is_empty() && selected.len() <= 100,
        "请选择 1 至 100 张目标表"
    );
    for target in selected {
        let source = request
            .prepared
            .tables
            .get(target.source)
            .context("源数据表已不存在")?;
        let dialect = crate::dialect::resolve(request.connection.kind())?;
        ensure!(
            target
                .source_key
                .as_ref()
                .is_none_or(|(origin, name)| origin == &source.origin && name == &source.name),
            "源数据表与映射配置不一致，请重新分析"
        );
        dialect.quote(&target.table.name)?;
        ensure!(!target.table.name.trim().is_empty(), "目标表名为空");
        let identity = if matches!(request.connection.kind(), "MYSQL" | "SQLITE") {
            format!("{}.{}", target.table.schema, target.table.name).to_lowercase()
        } else {
            format!("{}.{}", target.table.schema, target.table.name)
        };
        ensure!(
            destinations.insert(identity),
            "多个源表指向同一目标表，请分别导入"
        );
        let mut names = std::collections::HashSet::new();
        let mut keys = 0;
        let mut fields = 0;
        for f in target.fields.iter().filter(|f| f.enabled) {
            fields += 1;
            ensure!(
                f.source.is_none_or(|i| i < source.fields.len()),
                "源字段已不存在"
            );
            dialect.quote(&f.target)?;
            ensure!(!f.target.trim().is_empty(), "目标字段名为空");
            let name = if matches!(request.connection.kind(), "MYSQL" | "SQLITE") {
                f.target.to_lowercase()
            } else {
                f.target.clone()
            };
            ensure!(names.insert(name), "目标字段 {} 被重复映射", f.target);
            if target.create {
                f.data_type.sql(request.connection.kind(), f.key)?;
                ensure!(
                    !(f.key && f.data_type == DataType::Binary),
                    "二进制字段不能作为新表的导入键"
                );
            }
            if f.key {
                keys += 1;
                ensure!(
                    f.source.is_some() || f.default.is_some(),
                    "键字段 {} 没有源值或默认值",
                    f.target
                );
            }
        }
        ensure!(fields > 0, "目标表 {} 没有启用的字段", target.table.name);
        ensure!(
            !request.options.mode.needs_key() || keys > 0,
            "{}模式需要至少一个键字段",
            request.options.mode.label()
        );
        ensure!(
            !target.create || matches!(request.options.mode, Mode::Append | Mode::Copy),
            "新建表请选择追加模式"
        );
        ensure!(
            request.options.mode != Mode::Copy || request.options.transaction,
            "复制模式必须使用单表事务，以免取消后留下空表"
        );
    }
    Ok(())
}
pub fn create_sql(kind: &str, target: &Target) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    let mut columns = Vec::new();
    let mut keys = Vec::new();
    for f in target.fields.iter().filter(|f| f.enabled) {
        let name = dialect.quote(&f.target)?;
        let typ = f.data_type.sql(kind, f.key)?;
        columns.push(format!(
            "{name} {typ}{}",
            if !f.nullable || f.key {
                " NOT NULL"
            } else {
                ""
            }
        ));
        if f.key {
            keys.push(name)
        }
    }
    if !keys.is_empty() {
        columns.push(format!("PRIMARY KEY ({})", keys.join(", ")))
    }
    Ok(format!(
        "CREATE TABLE {} (\n  {}\n)",
        workbench::plan::qualified(kind, &target.table)?,
        columns.join(",\n  ")
    ))
}
pub fn run(request: Request, control: Control, runtime: tokio::runtime::Handle) -> Result<Report> {
    validate(&request)?;
    runtime.block_on(execute(request, &control))
}
async fn execute(request: Request, control: &Control) -> Result<Report> {
    let mut db = Databases::default();
    let conn = tokio::select! {r=db.session_in(&request.connection,&request.database)=>r?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    let kind = request.connection.kind();
    if kind == "MYSQL" {
        tokio::select! {r=conn.execute("SET SESSION sql_mode=CONCAT(@@session.sql_mode,',STRICT_ALL_TABLES')")=>{r?;},()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    }
    let mut report = Report::default();
    let count = request.targets.iter().filter(|t| t.selected).count();
    control.reset(count as u64);
    for target in request.targets.iter().filter(|t| t.selected) {
        control.log(&format!("目标表：{}", target.table.name));
        if let Err(error) = table(conn, kind, target, &request, control, &mut report).await {
            report.cancelled = control.cancelled();
            report.error = Some(if report.cancelled {
                if report.uncertain {
                    "操作已取消；存在未确认的写入，请检查目标数据后再重试".into()
                } else {
                    "操作已取消；未提交的事务已回滚，已提交批次保留".into()
                }
            } else {
                error.to_string()
            });
            control.log(report.error.as_deref().unwrap());
            break;
        }
        control.object();
    }
    Ok(report)
}
struct Rows {
    input: BufReader<File>,
    line: Vec<u8>,
    columns: usize,
    expected: u64,
    read: u64,
}
impl Rows {
    fn open(prepared: &Prepared, index: usize) -> Result<Self> {
        let table = prepared.tables.get(index).context("源数据已释放")?;
        ensure!(
            std::path::Path::new(&table.spool).components().count() == 1,
            "源缓存路径无效"
        );
        Ok(Self {
            input: BufReader::with_capacity(
                64 * 1024,
                File::open(prepared.directory.path().join(&table.spool))?,
            ),
            line: Vec::new(),
            columns: table.fields.len(),
            expected: table.rows,
            read: 0,
        })
    }
    fn next(&mut self) -> Result<Option<Vec<Cell>>> {
        self.line.clear();
        let n = Read::by_ref(&mut self.input)
            .take((MAX_ROW_BYTES + 2) as u64)
            .read_until(b'\n', &mut self.line)?;
        if n == 0 {
            ensure!(self.read == self.expected, "源缓存记录数不一致");
            return Ok(None);
        }
        ensure!(
            n <= MAX_ROW_BYTES + 1 && self.line.last() == Some(&b'\n'),
            "源缓存行不完整或超过 16 MiB"
        );
        let mut row: Vec<Cell> = serde_json::from_slice(&self.line)?;
        ensure!(row.len() <= self.columns, "源缓存列数不一致");
        row.resize(self.columns, Cell::Null);
        self.read += 1;
        Ok(Some(row))
    }
}
fn counters(report: &Report) -> (u64, u64, u64) {
    (report.inserted, report.updated, report.deleted)
}
fn restore(report: &mut Report, counters: (u64, u64, u64)) {
    report.inserted = counters.0;
    report.updated = counters.1;
    report.deleted = counters.2;
}
async fn table(
    conn: &mut AnyConnection,
    kind: &str,
    target: &Target,
    request: &Request,
    control: &Control,
    report: &mut Report,
) -> Result<()> {
    control.check()?;
    if target.create {
        let sql = create_sql(kind, target)?;
        tokio::select! {r=conn.execute(sql.as_str())=>{r.context("创建目标表失败（未替换已有同名表）")?;},()=control.wait_cancelled()=>{report.uncertain=true;report.changed.push(target.table.clone());anyhow::bail!("创建表操作未确认")}};
        report.changed.push(target.table.clone());
    }
    let definition = tokio::select! {r=workbench::describe(conn,kind,&target.table)=>r?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    ensure!(!definition.view, "目标对象是只读视图");
    if !target.create {
        let expected = target
            .expected
            .as_ref()
            .context("请先读取目标表结构再开始导入")?;
        ensure!(
            serde_json::to_vec(&expected.columns)? == serde_json::to_vec(&definition.columns)?,
            "目标表结构在预览后发生变化，请重新映射字段"
        );
    }
    ensure!(
        !request.options.transaction || definition.transactional,
        "目标表不支持事务，请选择分批提交；已写入的数据无法回滚"
    );
    ensure!(
        request.options.mode != Mode::Copy || definition.transactional,
        "复制模式需要支持事务的目标表"
    );
    for f in target.fields.iter().filter(|f| f.enabled) {
        let column = definition
            .columns
            .iter()
            .find(|c| c.name == f.target)
            .with_context(|| format!("目标字段已不存在：{}", f.target))?;
        ensure!(!column.generated, "生成字段 {} 不能写入", column.name);
    }
    let mut rows = Rows::open(&request.prepared, target.source)?;
    let mut first = true;
    let mut done = false;
    while !done {
        control.check()?;
        let before = counters(report);
        let limit = if request.options.transaction {
            usize::MAX
        } else {
            request.options.batch_size
        };
        if definition.transactional {
            let mut tx = tokio::select! {r=conn.begin()=>r?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
            let result = tokio::select! {result=consume(
                &mut tx,
                kind,
                target,
                &definition,
                request,
                control,
                report,
                &mut rows,
                limit,
                &mut first,
            )=>result,()=control.wait_cancelled()=>Err(anyhow::anyhow!("操作已取消"))};
            match result {
                Ok(end) => {
                    control.log("正在提交导入数据…");
                    match tokio::time::timeout(std::time::Duration::from_secs(10), tx.commit())
                        .await
                    {
                        Ok(Ok(())) => {
                            done = end;
                            let after = counters(report);
                            report.committed +=
                                after.0 - before.0 + after.1 - before.1 + after.2 - before.2;
                        }
                        _ => {
                            report.uncertain = true;
                            if !report.changed.contains(&target.table) {
                                report.changed.push(target.table.clone());
                            }
                            restore(report, before);
                            anyhow::bail!("提交未获得确认，请检查目标数据后再决定是否重试");
                        }
                    }
                }
                Err(error) => {
                    if !matches!(
                        tokio::time::timeout(std::time::Duration::from_secs(3), tx.rollback())
                            .await,
                        Ok(Ok(()))
                    ) {
                        report.uncertain = true;
                        if !report.changed.contains(&target.table) {
                            report.changed.push(target.table.clone());
                        }
                    }
                    restore(report, before);
                    return Err(error);
                }
            }
        } else {
            done = consume(
                conn,
                kind,
                target,
                &definition,
                request,
                control,
                report,
                &mut rows,
                limit,
                &mut first,
            )
            .await?;
        }
        if counters(report) != before && !report.changed.contains(&target.table) {
            report.changed.push(target.table.clone());
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn consume(
    conn: &mut AnyConnection,
    kind: &str,
    target: &Target,
    definition: &TableDefinition,
    request: &Request,
    control: &Control,
    report: &mut Report,
    rows: &mut Rows,
    limit: usize,
    first: &mut bool,
) -> Result<bool> {
    if *first && request.options.mode == Mode::Copy {
        let sql = format!(
            "DELETE FROM {}",
            workbench::plan::qualified(kind, &target.table)?
        );
        let result = tokio::select! {r=conn.execute(sql.as_str())=>r?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
        report.deleted += result.rows_affected();
    }
    *first = false;
    for _ in 0..limit {
        control.check()?;
        let Some(row) = rows.next()? else {
            return Ok(true);
        };
        report.read += 1;
        let savepoint = "tomato_import_row";
        if request.options.continue_on_error && definition.transactional {
            conn.execute(format!("SAVEPOINT {savepoint}").as_str())
                .await?;
        }
        let result = tokio::select! {r=write_row(conn,kind,target,definition,&row,&request.prepared.source,&request.options)=>r,()=control.wait_cancelled()=>{if !definition.transactional {report.uncertain=true;if !report.changed.contains(&target.table){report.changed.push(target.table.clone());}}anyhow::bail!("操作已取消")}};
        match result {
            Ok((inserted, updated, deleted, skipped)) => {
                report.inserted += inserted;
                report.updated += updated;
                report.deleted += deleted;
                report.skipped += skipped;
                if !definition.transactional {
                    report.committed += inserted + updated + deleted;
                    if inserted + updated + deleted > 0 && !report.changed.contains(&target.table) {
                        report.changed.push(target.table.clone());
                    }
                }
            }
            Err(error) => {
                report.failed += 1;
                let message = format!(
                    "{}：源第 {} 条记录，{}",
                    target.table.name,
                    rows.read,
                    safe_error(&error)
                );
                control.log(&message);
                if report.errors.len() < 200 {
                    report.errors.push(message.clone())
                }
                if !request.options.continue_on_error {
                    return Err(anyhow::anyhow!(message));
                }
                if definition.transactional {
                    let sql = if kind == "ORACLE" {
                        format!("ROLLBACK TO {savepoint}")
                    } else {
                        format!("ROLLBACK TO SAVEPOINT {savepoint}")
                    };
                    conn.execute(sql.as_str())
                        .await
                        .context("回滚错误记录失败")?;
                }
            }
        }
        if request.options.continue_on_error && definition.transactional {
            workbench::release_savepoint(conn, kind, savepoint).await?;
        }
        control.records(1);
    }
    Ok(false)
}
fn safe_error(error: &anyhow::Error) -> String {
    if let Some(e) = error.downcast_ref::<sqlx::Error>() {
        if let Some(db) = e.as_database_error() {
            let code = db.code().map_or_else(|| "未知".into(), |c| c.into_owned());
            return format!("数据库拒绝此记录（SQLSTATE/错误码 {code}），请检查键、约束和字段类型");
        }
        return "数据库连接或写入失败".into();
    }
    error.to_string()
}
async fn write_row(
    conn: &mut AnyConnection,
    kind: &str,
    target: &Target,
    definition: &TableDefinition,
    row: &[Cell],
    source: &Source,
    options: &Options,
) -> Result<(u64, u64, u64, u64)> {
    let dialect = crate::dialect::resolve(kind)?;
    let mut values = vec![None; definition.columns.len()];
    let mut keys = Vec::new();
    let mut assigned = Vec::new();
    for f in target
        .fields
        .iter()
        .filter(|f| f.enabled && (options.mode != Mode::Delete || f.key))
    {
        let index = definition
            .columns
            .iter()
            .position(|c| c.name == f.target)
            .context("目标字段已不存在")?;
        let column = &definition.columns[index];
        let raw = if let Some(i) = f.source {
            Some(row.get(i).cloned().context("源字段已不存在")?)
        } else {
            f.default.as_ref().map(|v| Cell::Text(v.clone()))
        };
        let Some(raw) = raw else { continue };
        if raw == Cell::Null && column.auto_increment {
            continue;
        }
        let typ = if target.create {
            f.data_type.clone()
        } else {
            column_type(column)
        };
        let value = convert::value(&typ, raw, source, options)
            .with_context(|| format!("字段 {} 的数据类型无效", column.name))?;
        ensure!(
            kind != "SQLITE"
                || typ != DataType::Integer
                || matches!(value, Cell::Integer(_) | Cell::Null),
            "SQLite 整数超出有符号 64 位范围，请改用文本或数值文本字段"
        );
        ensure!(
            value != Cell::Null || column.nullable,
            "字段 {} 不允许 NULL",
            column.name
        );
        values[index] = Some(if kind == "ORACLE" && typ == DataType::Date {
            match value {
                Cell::Text(s) => Cell::Text(format!("{s} 00:00:00.000")),
                v => v,
            }
        } else {
            value
        });
        assigned.push(index);
        if f.key {
            keys.push(index)
        }
    }
    let mut exists = false;
    let table = workbench::plan::qualified(kind, &target.table)?;
    if options.mode.needs_key() {
        ensure!(!keys.is_empty(), "导入键为空或自动编号键值为空");
        let mut params = Vec::new();
        let predicate = predicate(kind, definition, &values, &keys, &mut params)?;
        let suffix = if kind == "ORACLE" {
            "FETCH FIRST 2 ROWS ONLY"
        } else {
            "LIMIT 2"
        };
        let sql = format!("SELECT 1 FROM {table} WHERE {predicate} {suffix}");
        let matches = workbench::query(&sql, &params)
            .fetch_all(&mut *conn)
            .await?;
        ensure!(matches.len() <= 1, "导入键不能唯一定位目标记录");
        exists = !matches.is_empty();
    }
    if options.mode == Mode::SkipDuplicates && exists
        || options.mode == Mode::Update && !exists
        || options.mode == Mode::Delete && !exists
    {
        return Ok((0, 0, 0, 1));
    }
    if options.mode == Mode::Delete
        || exists && matches!(options.mode, Mode::Update | Mode::AppendUpdate)
    {
        let mut params = Vec::new();
        let mut sets = Vec::new();
        if options.mode != Mode::Delete {
            for i in assigned.iter().filter(|i| !keys.contains(i)) {
                let column = &definition.columns[*i];
                let value = values[*i].as_ref().unwrap();
                sets.push(format!(
                    "{}={}",
                    dialect.quote(&column.name)?,
                    workbench::plan::argument(kind, column, value, &mut params)?
                ));
            }
            if sets.is_empty() {
                return Ok((0, 0, 0, 1));
            }
        }
        let predicate = predicate(kind, definition, &values, &keys, &mut params)?;
        let sql = if options.mode == Mode::Delete {
            format!("DELETE FROM {table} WHERE {predicate}")
        } else {
            format!("UPDATE {table} SET {} WHERE {predicate}", sets.join(","))
        };
        let affected = workbench::query(&sql, &params)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        ensure!(affected <= 1, "目标记录在导入期间发生变化");
        return if options.mode == Mode::Delete {
            Ok((0, 0, affected, 0))
        } else {
            Ok((0, 1, 0, 0))
        };
    }
    if let Some(statement) =
        workbench::plan::build(kind, definition, &RowChange::Insert { values })?
    {
        workbench::query(&statement.sql, &statement.params)
            .execute(&mut *conn)
            .await?;
        Ok((1, 0, 0, 0))
    } else {
        Ok((0, 0, 0, 1))
    }
}
fn predicate(
    kind: &str,
    definition: &TableDefinition,
    values: &[Option<Cell>],
    keys: &[usize],
    params: &mut Vec<Cell>,
) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    let mut parts = Vec::new();
    for &i in keys {
        let column = &definition.columns[i];
        let value = values[i].as_ref().context("导入键没有值")?;
        let name = dialect.quote(&column.name)?;
        parts.push(if *value == Cell::Null {
            format!("{name} IS NULL")
        } else {
            format!(
                "{name}={}",
                workbench::plan::argument(kind, column, value, params)?
            )
        });
    }
    ensure!(!parts.is_empty(), "导入键不能为空");
    Ok(parts.join(" AND "))
}
