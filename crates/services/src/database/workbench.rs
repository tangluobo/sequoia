use super::Databases;
use anyhow::{Context, Result, bail};
use futures_util::TryStreamExt;
use sqlx::{
    Any, AnyConnection, Column as _, Connection as _, Executor, Row, ValueRef,
    any::{AnyArguments, AnyRow},
    query::Query,
};
use tomato_core::workbench::*;
use tomato_core::{Connection, limits};
#[path = "plan.rs"]
pub(super) mod plan;
pub use plan::inserts_sql;
pub(super) mod catalog;
mod completion;
pub(crate) mod criteria;
mod read;

pub(super) fn query<'a>(sql: &'a str, values: &[Cell]) -> Query<'a, Any, AnyArguments<'a>> {
    let mut query = sqlx::query(sql);
    for value in values {
        query = match value {
            Cell::Null => query.bind(Option::<String>::None),
            Cell::Text(value) => query.bind(value.clone()),
            Cell::Integer(value) => query.bind(*value),
            Cell::Real(value) => query.bind(*value),
            Cell::Boolean(value) => query.bind(*value),
            Cell::Binary(value) => query.bind(value.clone()),
        };
    }
    query
}
pub(super) async fn metadata(
    conn: &mut AnyConnection,
    sql: &str,
    values: &[Cell],
) -> Result<Vec<AnyRow>> {
    let mut stream = query(sql, values).fetch(conn);
    let mut rows = Vec::new();
    let mut held = 0usize;
    while let Some(row) = stream.try_next().await? {
        if rows.len() >= 10_000 {
            bail!("对象超过 10,000 项，请缩小数据库范围");
        }
        let row_bytes = std::mem::size_of::<AnyRow>()
            + row.values.capacity() * std::mem::size_of::<sqlx_core::any::AnyValue>()
            + row.columns.capacity() * std::mem::size_of::<sqlx_core::any::AnyColumn>()
            + row
                .values
                .iter()
                .map(|v| match &v.kind {
                    sqlx_core::any::AnyValueKind::Text(v) => v.len(),
                    sqlx_core::any::AnyValueKind::Blob(v) => v.len(),
                    _ => 0,
                })
                .sum::<usize>();
        held = held.saturating_add(row_bytes);
        if held > limits::QUERY_BYTES {
            bail!("结构元数据超过 8 MiB，请缩小数据库范围");
        }
        rows.push(row);
    }
    Ok(rows)
}
pub(super) fn text(row: &AnyRow, index: usize) -> Result<String> {
    // MySQL can advertise long textual expressions/metadata as BLOB. These
    // callers select text explicitly; binary table cells are HEX-encoded first.
    if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
        return Ok(String::from_utf8(value)?);
    }
    super::cell(row, index).map(Option::unwrap_or_default)
}
pub(super) fn optional(row: &AnyRow, index: usize) -> Result<Option<String>> {
    if row.try_get_raw(index)?.is_null() {
        Ok(None)
    } else {
        Ok(Some(text(row, index)?))
    }
}
fn flag(row: &AnyRow, index: usize) -> bool {
    text(row, index).is_ok_and(|text| text == "1" || text == "true" || text == "YES")
}
pub(super) fn strings(row: &AnyRow, index: usize) -> Result<Cell> {
    if row.try_get_raw(index)?.is_null() {
        Ok(Cell::Null)
    } else {
        Ok(Cell::Text(text(row, index)?))
    }
}
pub(super) fn native_cell(row: &AnyRow, index: usize) -> Result<Cell> {
    if row.try_get_raw(index)?.is_null() {
        return Ok(Cell::Null);
    }
    if let Ok(value) = row.try_get::<String, _>(index) {
        return Ok(Cell::Text(value));
    }
    if let Ok(value) = row.try_get::<i64, _>(index) {
        return Ok(Cell::Integer(value));
    }
    if let Ok(value) = row.try_get::<i32, _>(index) {
        return Ok(Cell::Integer(value.into()));
    }
    if let Ok(value) = row.try_get::<f64, _>(index) {
        return Ok(Cell::Real(value));
    }
    if let Ok(value) = row.try_get::<bool, _>(index) {
        return Ok(Cell::Boolean(value));
    }
    if let Ok(value) = row.try_get::<Vec<u8>, _>(index) {
        return Ok(Cell::Binary(value));
    }
    bail!("字段类型无法解码：{}", row.columns()[index].name())
}
pub(super) fn unhex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("二进制字段的十六进制长度无效");
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |value: u8| {
                (value as char)
                    .to_digit(16)
                    .context("二进制字段不是有效十六进制")
            };
            Ok(((digit(pair[0])? << 4) | digit(pair[1])?) as u8)
        })
        .collect()
}

pub(super) async fn describe(
    conn: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<TableDefinition> {
    if kind == "ORACLE" {
        return super::oracle::catalog::describe(conn, table).await;
    }
    if kind == "HIVE" {
        return super::hive::catalog::describe(conn, table).await;
    }
    let dialect = crate::dialect::resolve(kind)?;
    let mut columns = Vec::new();
    let mut comments = std::collections::BTreeMap::new();
    let mut comment_bytes = 0usize;
    let mut view = false;
    let mut transactional = true;
    match kind {
        "SQLITE" => {
            let sql = format!(
                "PRAGMA {}.table_xinfo({})",
                dialect.quote(&table.schema)?,
                dialect.quote(&table.name)?
            );
            for row in metadata(conn, &sql, &[]).await? {
                let data_type = text(&row, 2)?;
                let hidden = text(&row, 6)?.parse::<u32>()?;
                if hidden == 1 {
                    continue;
                }
                columns.push(Column {
                    name: text(&row, 1)?,
                    binary: data_type.to_ascii_uppercase().contains("BLOB"),
                    data_type,
                    nullable: !flag(&row, 3),
                    default: optional(&row, 4)?,
                    primary_key: text(&row, 5)?.parse()?,
                    generated: hidden > 1,
                    auto_increment: false,
                    type_schema: String::new(),
                    type_name: String::new(),
                });
            }
            let sql = format!(
                "SELECT type FROM {}.sqlite_master WHERE name=?",
                dialect.quote(&table.schema)?
            );
            if let Some(row) = query(&sql, &[Cell::Text(table.name.clone())])
                .fetch_optional(&mut *conn)
                .await?
            {
                view = text(&row, 0)? == "view";
            }
            // A single INTEGER PRIMARY KEY is an implicit rowid default too.
            if columns
                .iter()
                .filter(|column| column.primary_key > 0)
                .count()
                == 1
            {
                for column in &mut columns {
                    if column.primary_key > 0 && column.data_type.eq_ignore_ascii_case("INTEGER") {
                        column.auto_increment = true;
                    }
                }
            }
        }
        "MYSQL" => {
            let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, COLUMN_KEY, EXTRA, DATA_TYPE, COLUMN_COMMENT FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION";
            for row in metadata(
                conn,
                sql,
                &[
                    Cell::Text(table.schema.clone()),
                    Cell::Text(table.name.clone()),
                ],
            )
            .await?
            {
                let comment = text(&row, 7)?;
                if !comment.is_empty() {
                    let name = text(&row, 0)?;
                    comment_bytes += name.len() + comment.len() + 128;
                    anyhow::ensure!(comment_bytes <= 256 * 1024, "列注释超过 256 KiB");
                    comments.insert(name, comment);
                }
                let extra = text(&row, 5)?.to_ascii_lowercase();
                let data_type = text(&row, 6)?.to_ascii_lowercase();
                columns.push(Column {
                    name: text(&row, 0)?,
                    data_type: text(&row, 1)?,
                    nullable: text(&row, 2)? == "YES",
                    default: optional(&row, 3)?,
                    primary_key: u32::from(text(&row, 4)? == "PRI"),
                    generated: extra.contains("generated") && !extra.contains("default_generated"),
                    auto_increment: extra.contains("auto_increment"),
                    binary: [
                        "binary",
                        "varbinary",
                        "tinyblob",
                        "blob",
                        "mediumblob",
                        "longblob",
                        "bit",
                    ]
                    .contains(&data_type.as_str()),
                    type_schema: String::new(),
                    type_name: String::new(),
                });
            }
            if let Some(row) = query("SELECT TABLE_TYPE, COALESCE(ENGINE,'') FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?", &[Cell::Text(table.schema.clone()), Cell::Text(table.name.clone())]).fetch_optional(&mut *conn).await? {
                view = text(&row, 0)?.contains("VIEW");
                transactional = ["innodb", "ndb", "ndbcluster"].contains(&text(&row, 1)?.to_ascii_lowercase().as_str());
            }
        }
        "POSTGRESQL" => {
            let sql = "SELECT a.attname::text, format_type(a.atttypid,a.atttypmod), CASE WHEN a.attnotnull THEN 0::bigint ELSE 1::bigint END, pg_get_expr(d.adbin,d.adrelid), CASE WHEN EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary AND a.attnum=ANY(i.indkey)) THEN 1::bigint ELSE 0::bigint END, CASE WHEN a.attgenerated<>'' THEN 1::bigint ELSE 0::bigint END, CASE WHEN a.attidentity<>'' THEN 1::bigint ELSE 0::bigint END, tn.nspname::text, t.typname::text, c.relkind::text, COALESCE(col_description(c.oid,a.attnum),'')::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid JOIN pg_namespace tn ON tn.oid=t.typnamespace LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum";
            for row in metadata(
                conn,
                sql,
                &[
                    Cell::Text(table.schema.clone()),
                    Cell::Text(table.name.clone()),
                ],
            )
            .await?
            {
                let comment = text(&row, 10)?;
                if !comment.is_empty() {
                    let name = text(&row, 0)?;
                    comment_bytes += name.len() + comment.len() + 128;
                    anyhow::ensure!(comment_bytes <= 256 * 1024, "列注释超过 256 KiB");
                    comments.insert(name, comment);
                }
                let default = optional(&row, 3)?;
                let type_name = text(&row, 8)?;
                view = ["v", "m"].contains(&text(&row, 9)?.as_str());
                columns.push(Column {
                    name: text(&row, 0)?,
                    data_type: text(&row, 1)?,
                    nullable: flag(&row, 2),
                    primary_key: u32::from(flag(&row, 4)),
                    generated: flag(&row, 5),
                    auto_increment: flag(&row, 6)
                        || default
                            .as_ref()
                            .is_some_and(|value| value.starts_with("nextval(")),
                    default,
                    binary: type_name == "bytea",
                    type_schema: text(&row, 7)?,
                    type_name,
                });
            }
        }
        _ => bail!("此数据库尚无表结构适配器"),
    }
    if columns.is_empty() {
        bail!("表不存在、没有可见列或无读取权限");
    }
    Ok(TableDefinition {
        comments,
        table: table.clone(),
        columns,
        view,
        transactional,
    })
}

/// Compatibility projection for streaming export/transfer, not interactive table reads.
pub(super) fn projection(kind: &str, column: &Column) -> Result<String> {
    let name = crate::dialect::resolve(kind)?.quote(&column.name)?;
    Ok(match (kind, column.binary) {
        // BIT is a numeric HEX argument: HEX(1) is "1", losing both byte
        // alignment and declared width. Force its original binary-string value.
        ("MYSQL", true) => format!("HEX(CAST({name} AS BINARY))"),
        ("MYSQL", false) => format!("CAST({name} AS CHAR)"),
        ("POSTGRESQL", true) => format!("encode({name}, 'hex')"),
        ("POSTGRESQL", false) => format!("CAST({name} AS TEXT)"),
        _ => name,
    })
}

/// The exact statement executed by the table loader, also displayed in its status bar.
pub fn page_sql(kind: &str, definition: &TableDefinition, request: &PageRequest) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    let table = plan::qualified(kind, &request.table)?;
    let limit = request.limit.clamp(1, 1000);
    let selections = definition
        .columns
        .iter()
        .map(|column| dialect.quote(&column.name))
        .collect::<Result<Vec<_>>>()?;
    let (predicate, sort) = criteria::clauses(kind, definition, &criteria::effective(request))?;
    if kind == "ORACLE" {
        return Ok(format!(
            "SELECT {} FROM {table}{predicate}{sort} OFFSET {} ROWS FETCH NEXT {limit} ROWS ONLY",
            selections.join(", "),
            request.offset
        ));
    }
    if kind == "HIVE" {
        return Ok(format!(
            "SELECT {} FROM {table}{predicate}{sort} LIMIT {},{limit}",
            selections.join(", "),
            request.offset
        ));
    }
    Ok(format!(
        "SELECT {} FROM {table}{predicate}{sort} LIMIT {limit} OFFSET {}",
        selections.join(", "),
        request.offset
    ))
}

async fn page(conn: &mut AnyConnection, kind: &str, mut request: PageRequest) -> Result<TablePage> {
    let definition = describe(conn, kind, &request.table).await?;
    let table = plan::qualified(kind, &request.table)?;
    let criteria = criteria::effective(&request);
    let (predicate, _) = criteria::clauses(kind, &definition, &criteria)?;
    let count = query(&format!("SELECT COUNT(*) FROM {table}{predicate}"), &[])
        .fetch_one(&mut *conn)
        .await?;
    let total = text(&count, 0)?.parse::<u64>()?;
    let limit = request.limit.clamp(1, 1000);
    if request.offset >= total {
        request.offset = total.saturating_sub(1) / u64::from(limit) * u64::from(limit);
    }
    let sql = page_sql(kind, &definition, &request)?;
    let read::Rows { rows, truncated } = read::page(conn, kind, &sql, &definition.columns).await?;
    Ok(TablePage {
        definition,
        rows,
        total,
        offset: request.offset,
        limit,
        truncated,
        query: criteria,
        sql,
    })
}

pub(super) async fn writes(
    conn: &mut AnyConnection,
    statements: &[plan::Statement],
) -> Result<u64> {
    let mut affected = 0;
    for statement in statements {
        let count = query(&statement.sql, &statement.params)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        if count != 1 {
            bail!("记录已被其他操作修改或无法唯一定位（影响 {count} 行）；本批修改已取消");
        }
        affected += count;
    }
    Ok(affected)
}

fn code(error: &sqlx::Error, expected: &str) -> bool {
    error.as_database_error().is_some_and(|error| {
        error.code().is_some_and(|code| code == expected)
            || error
                .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                .is_some_and(|error| {
                    expected
                        .parse::<u16>()
                        .is_ok_and(|number| number == error.number())
                })
    })
}

pub(super) fn savepoint_name(kind: &str, purpose: &str) -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    // Oracle 12.1 limits identifiers to 30 bytes. Generated names are never
    // user identifiers and only need a collision-resistant session-local suffix.
    format!(
        "tomato_{purpose}_{}",
        if kind == "ORACLE" {
            &random[..12]
        } else {
            &random
        }
    )
}

pub(super) async fn release_savepoint(
    conn: &mut AnyConnection,
    kind: &str,
    name: &str,
) -> Result<()> {
    // Oracle has ROLLBACK TO but no RELEASE SAVEPOINT statement.
    if kind != "ORACLE" {
        conn.execute(format!("RELEASE SAVEPOINT {name}").as_str())
            .await?;
    }
    Ok(())
}

pub(super) async fn existing_savepoint(
    conn: &mut AnyConnection,
    kind: &str,
    name: &str,
) -> Result<bool> {
    if kind == "HIVE" || kind == "ORACLE" && !conn.is_in_transaction() {
        return Ok(false);
    }
    match conn.execute(format!("SAVEPOINT {name}").as_str()).await {
        Ok(_) if kind == "MYSQL" => {
            match conn
                .execute(format!("RELEASE SAVEPOINT {name}").as_str())
                .await
            {
                Ok(_) => {
                    conn.execute(format!("SAVEPOINT {name}").as_str()).await?;
                    Ok(true)
                }
                Err(error) if code(&error, "1305") => Ok(false),
                Err(error) => Err(error.into()),
            }
        }
        Ok(_) => Ok(true),
        Err(error) if kind == "POSTGRESQL" && code(&error, "25P01") => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn apply(
    conn: &mut AnyConnection,
    kind: &str,
    definition: &TableDefinition,
    changes: &[RowChange],
) -> Result<u64> {
    if changes.len() > 1000 {
        bail!("每批最多保存 1,000 行修改");
    }
    let plans = changes
        .iter()
        .map(|change| plan::build(kind, definition, change))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if plans.is_empty() {
        return Ok(0);
    }
    let name = savepoint_name(kind, "rows");
    if existing_savepoint(conn, kind, &name).await? {
        let result = writes(conn, &plans).await;
        if result.is_err() {
            conn.execute(format!("ROLLBACK TO SAVEPOINT {name}").as_str())
                .await
                .context("回滚本批修改失败")?;
        }
        if let Err(error) = release_savepoint(conn, kind, &name).await {
            // SQLite can defer foreign-key validation until the outermost
            // RELEASE. A failed commit leaves that savepoint and its writes live.
            conn.execute(format!("ROLLBACK TO SAVEPOINT {name}").as_str())
                .await
                .context("保存点释放失败且无法回滚，请关闭此连接的所有标签后重新连接")?;
            conn.execute(format!("RELEASE SAVEPOINT {name}").as_str())
                .await?;
            return Err(error).context("本批修改未提交，已回滚");
        }
        result
    } else {
        let mut transaction = conn.begin().await?;
        let affected = writes(&mut transaction, &plans).await?;
        transaction.commit().await?;
        Ok(affected)
    }
}

impl Databases {
    pub(super) async fn workbench_once(
        &mut self,
        connection: &Connection,
        action: DatabaseAction,
    ) -> Result<DatabaseOutput> {
        let kind = connection.kind();
        let database = if matches!(kind, "MYSQL" | "HIVE") {
            match &action {
                DatabaseAction::Objects { schema }
                | DatabaseAction::ObjectHeaders { schema }
                | DatabaseAction::ObjectFields { schema, .. }
                | DatabaseAction::ObjectMutation { schema, .. }
                | DatabaseAction::Completion { schema }
                | DatabaseAction::Tables { schema }
                | DatabaseAction::SelectSchema { schema } => schema.as_str(),
                DatabaseAction::Page { request } | DatabaseAction::Apply { request, .. } => {
                    request.table.schema.as_str()
                }
                DatabaseAction::Definition { table }
                | DatabaseAction::DeleteRows { table, .. }
                | DatabaseAction::Design { table }
                | DatabaseAction::DesignStructure { table }
                | DatabaseAction::DesignSql { table }
                | DatabaseAction::DesignInfo { table, .. } => table.schema.as_str(),
                DatabaseAction::SaveDesign { edit } | DatabaseAction::SaveStructure { edit } => {
                    edit.after.table.schema.as_str()
                }
                DatabaseAction::CreateTable { request } => request.table.schema.as_str(),
                _ => connection.text("database"),
            }
        } else {
            connection.text("database")
        };
        let conn = self.session_in(connection, database).await?;
        let action = if let DatabaseAction::SelectSchema { schema } = action {
            let quoted = crate::dialect::resolve(kind)?.quote(&schema)?;
            match kind {
                "MYSQL" | "HIVE" => {
                    conn.execute(format!("USE {quoted}").as_str()).await?;
                }
                "POSTGRESQL" => {
                    conn.execute(format!("SET search_path TO {quoted}").as_str())
                        .await?;
                }
                _ => {}
            }
            DatabaseAction::Tables { schema }
        } else {
            action
        };
        match action {
            DatabaseAction::Design { table } => Ok(DatabaseOutput::Design(Box::new(
                super::objects::designer::read(conn, kind, &table).await?,
            ))),
            DatabaseAction::DesignStructure { table } => Ok(DatabaseOutput::Design(Box::new(
                super::objects::designer::read_structure(conn, kind, &table).await?,
            ))),
            DatabaseAction::DesignSql { table } => Ok(DatabaseOutput::DesignSql {
                sql: super::objects::designer::read_sql(conn, kind, &table).await?,
                table,
            }),
            DatabaseAction::SaveDesign { edit } => Ok(DatabaseOutput::Design(Box::new(
                super::objects::designer::save(
                    conn,
                    kind,
                    *edit,
                    super::objects::designer::Detail::WithSql,
                )
                .await?,
            ))),
            DatabaseAction::SaveStructure { edit } => Ok(DatabaseOutput::Design(Box::new(
                super::objects::designer::save(
                    conn,
                    kind,
                    *edit,
                    super::objects::designer::Detail::Structure,
                )
                .await?,
            ))),
            DatabaseAction::DesignInfo { table, page } => Ok(DatabaseOutput::DesignInfo {
                page,
                info: crate::database::objects::designer::read_info(conn, kind, &table, page)
                    .await?,
            }),
            DatabaseAction::DesignMetadata => Ok(DatabaseOutput::DesignMetadata(
                super::objects::design_metadata(conn, kind).await?,
            )),
            DatabaseAction::CreateTable { request } => Ok(DatabaseOutput::TableCreated(
                super::objects::create_table(conn, kind, request).await?,
            )),
            DatabaseAction::Objects { schema } => Ok(DatabaseOutput::Objects(
                super::objects::read(conn, kind, &schema).await?,
            )),
            DatabaseAction::ObjectHeaders { schema } => Ok(DatabaseOutput::ObjectHeaders(
                super::objects::headers(conn, kind, &schema).await?,
            )),
            DatabaseAction::ObjectFields { schema, catalog } => {
                let fields = super::objects::preload(conn, kind, &schema, &catalog).await?;
                Ok(DatabaseOutput::ObjectFields { schema, fields })
            }
            DatabaseAction::ObjectMutation { schema, request } => {
                Ok(DatabaseOutput::ObjectsChanged(
                    super::objects::change(conn, kind, &schema, request).await?,
                ))
            }
            DatabaseAction::Completion { schema } => Ok(DatabaseOutput::Completion(
                completion::read(conn, kind, &schema, connection.text("database")).await?,
            )),
            DatabaseAction::SelectSchema { .. } => {
                unreachable!("schema selection normalized above")
            }
            DatabaseAction::Databases => Ok(DatabaseOutput::Databases(
                catalog::databases(conn, kind).await?,
            )),
            DatabaseAction::Schemas => Ok(DatabaseOutput::Schemas(
                catalog::namespaces(conn, kind).await?,
            )),
            DatabaseAction::Tables { schema } => {
                if kind == "HIVE" {
                    return Ok(DatabaseOutput::Tables(
                        super::hive::catalog::tables(conn, &schema).await?,
                    ));
                }
                let (sql, values) = match kind {
                    "MYSQL" => ("SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME".into(), vec![Cell::Text(schema.clone())]),
                    "POSTGRESQL" => ("SELECT table_name::text,table_type::text FROM information_schema.tables WHERE table_schema=$1 ORDER BY table_name".into(), vec![Cell::Text(schema.clone())]),
                    "ORACLE" => (super::oracle::catalog::TABLES.into(),vec![Cell::Text(schema.clone())]),
                    _ => (format!("SELECT name,type FROM {}.sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name", crate::dialect::resolve(kind)?.quote(&schema)?), vec![]),
                };
                let tables = metadata(conn, &sql, &values)
                    .await?
                    .iter()
                    .map(|row| {
                        Ok(TableInfo {
                            table: TableId {
                                schema: schema.clone(),
                                name: text(row, 0)?,
                            },
                            view: text(row, 1)?.to_ascii_lowercase().contains("view"),
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(DatabaseOutput::Tables(tables))
            }
            DatabaseAction::Definition { table } => Ok(DatabaseOutput::Definition(
                describe(conn, kind, &table).await?,
            )),
            DatabaseAction::Page { request } => {
                Ok(DatabaseOutput::Page(page(conn, kind, request).await?))
            }
            DatabaseAction::DeleteRows { table, rows } => {
                let definition = describe(conn, kind, &table).await?;
                let (indices, changes): (Vec<_>, Vec<_>) = rows
                    .into_iter()
                    .map(|(index, original)| (index, RowChange::Delete { original }))
                    .unzip();
                let affected = apply(conn, kind, &definition, &changes).await?;
                let rows = indices
                    .into_iter()
                    .zip(changes.into_iter().map(|change| {
                        let RowChange::Delete { original } = change else {
                            unreachable!()
                        };
                        original
                    }))
                    .collect();
                Ok(DatabaseOutput::Deleted {
                    table,
                    rows,
                    affected,
                })
            }
            DatabaseAction::Apply {
                table,
                changes,
                request,
            } => {
                if table != request.table {
                    bail!("保存目标与刷新目标不一致");
                }
                let definition = describe(conn, kind, &table).await?;
                let affected = apply(conn, kind, &definition, &changes).await?;
                // A refresh failure must not be reported as a failed write that can
                // safely be retried. Obtain the refreshed page in the same command.
                let (page, refresh_error) = match page(conn, kind, request).await {
                    Ok(page) => (Some(page), None),
                    Err(error) => (None, Some(format!("修改已保存，但刷新失败：{error}"))),
                };
                Ok(DatabaseOutput::Applied {
                    affected,
                    page,
                    refresh_error,
                })
            }
            DatabaseAction::Transaction { action } => {
                match action {
                    TransactionAction::Begin => {
                        if kind == "MYSQL" {
                            let name = format!("tomato_probe_{}", uuid::Uuid::new_v4().simple());
                            if existing_savepoint(conn, kind, &name).await? {
                                conn.execute(format!("RELEASE SAVEPOINT {name}").as_str())
                                    .await?;
                                bail!("当前已有事务，请先提交或回滚");
                            }
                        }
                        conn.execute(if kind == "MYSQL" {
                            "START TRANSACTION"
                        } else {
                            "BEGIN"
                        })
                        .await?;
                    }
                    TransactionAction::Commit => {
                        conn.execute("COMMIT").await?;
                    }
                    TransactionAction::Rollback => {
                        conn.execute("ROLLBACK").await?;
                    }
                }
                Ok(DatabaseOutput::Transaction(matches!(
                    action,
                    TransactionAction::Begin
                )))
            }
        }
    }
}
