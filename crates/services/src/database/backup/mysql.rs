//! Native MySQL backup strategy. SQL payloads borrow wire bytes and spool gzip pages to disk.
use super::super::workbench;
use crate::backup::{Control, DefinitionPage, Page, Writer};
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt;
use sqlx::{AnyConnection, Column as _, Row, TypeInfo, ValueRef};
use std::path::Path;
use tomato_core::{
    Connection,
    backup::{self, Definition, Kind, Metadata, Object, Selection, sql::Value},
};
fn qualified(schema: &str, name: &str) -> Result<String> {
    let dialect = crate::dialect::resolve("MYSQL")?;
    Ok(format!(
        "{}.{}",
        dialect.quote(schema)?,
        dialect.quote(name)?
    ))
}
async fn ddl(conn: &mut AnyConnection, selection: &Selection) -> Result<String> {
    let (kind, column) = match selection.kind {
        Kind::Table => ("TABLE", "Create Table"),
        Kind::View => ("VIEW", "Create View"),
        Kind::Function => ("FUNCTION", "Create Function"),
        Kind::Event => ("EVENT", "Create Event"),
    };
    let sql = format!(
        "SHOW CREATE {kind} {}",
        qualified(&selection.table.schema, &selection.table.name)?
    );
    let row = sqlx::query(&sql).fetch_one(conn).await?;
    let index = row
        .columns()
        .iter()
        .position(|col| col.name() == column)
        .context("数据库未返回对象定义")?;
    let ddl = workbench::text(&row, index)?;
    ensure!(
        !ddl.is_empty() && ddl.len() <= 8 * 1024 * 1024,
        "对象定义为空或超过 8 MiB"
    );
    Ok(ddl)
}
pub(super) async fn objects(
    conn: &mut AnyConnection,
    connection: &Connection,
    request: &backup::Create,
    directory: &Path,
    control: &Control,
) -> Result<(Writer, Metadata, u64)> {
    let mut writer = Writer::new(directory)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let mut meta = Metadata {
        meta_version: "30100".into(),
        database_type: connection.kind().into(),
        catalog: String::new(),
        schema: request.scope.database.clone(),
        start_time: now.clone(),
        end_time: now,
        encryption: "None".into(),
        comment: request.comment.clone(),
        objects: Vec::new(),
    };
    let mut records = 0u64;
    control.start(request.objects.len() as u64);
    for selection in &request.objects {
        control.check()?;
        control.log(&format!(
            "处理 {}: {}",
            selection.kind.label(),
            selection.table.name
        ));
        let mut selected = selection.clone();
        if selected.table.schema.is_empty() {
            selected.table.schema = request.scope.database.clone();
        }
        ensure!(
            selected.table.schema == request.scope.database,
            "备份对象不属于当前数据库"
        );
        let uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let table = if selected.kind == Kind::Table {
            Some(workbench::describe(conn, "MYSQL", &selected.table).await?)
        } else {
            None
        };
        let fields = table.as_ref().map_or_else(Vec::new, |definition| {
            definition
                .columns
                .iter()
                .filter(|column| !column.generated)
                .map(|column| column.name.clone())
                .collect::<Vec<_>>()
        });
        let definition = Definition {
            meta_version: "30100".into(),
            name: selected.table.name.clone(),
            kind: selected.kind.archive_name().into(),
            ddl: ddl(conn, &selected).await?,
            sub_ddl: Some(Vec::new()),
            auto_increment: Some(String::new()),
            fields: Some(fields.clone()),
            trigger_ddl: Some(Vec::new()),
            index_ddl: Some(Vec::new()),
            data: None,
            postgres: None,
            oracle: None,
        };
        let mut definition_page = DefinitionPage::new(directory, &definition)?;
        let mut count = 0u64;
        if selected.kind == Kind::Table {
            let dialect = crate::dialect::resolve("MYSQL")?;
            let columns = if fields.is_empty() {
                "1".to_owned()
            } else {
                fields
                    .iter()
                    .map(|field| dialect.quote(field))
                    .collect::<Result<Vec<_>>>()?
                    .join(",")
            };
            let sql = format!(
                "SELECT {columns} FROM {}",
                qualified(&selected.table.schema, &selected.table.name)?
            );
            let native = conn
                .backend_mut::<sqlx::MySqlConnection>()
                .context("MySQL 会话类型不匹配")?;
            let mut rows = sqlx::raw_sql(&sql).fetch(native);
            let mut page = Page::new(directory)?;
            let mut page_number = 0u64;
            while let Some(row) = rows.try_next().await? {
                control.check()?;
                page.record(|output| {
                    output.write_all(b"(")?;
                    for (index, _) in fields.iter().enumerate() {
                        if index > 0 {
                            output.write_all(b", ")?;
                        }
                        let value = row.try_get_raw(index)?;
                        if value.is_null() {
                            backup::sql::literal(output, "MYSQL", Value::Null)?;
                            continue;
                        }
                        let bytes = value
                            .as_bytes()
                            .map_err(|error| anyhow::anyhow!("备份字段读取失败: {error}"))?;
                        let kind = row.column(index).type_info().name();
                        if kind == "BIT" {
                            ensure!(bytes.len() <= 8, "BIT 值超过 64 位");
                            let number = bytes
                                .iter()
                                .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
                                .to_string();
                            backup::sql::literal(output, "MYSQL", Value::Number(&number))?;
                        } else if matches!(
                            kind,
                            "BINARY"
                                | "VARBINARY"
                                | "TINYBLOB"
                                | "BLOB"
                                | "MEDIUMBLOB"
                                | "LONGBLOB"
                        ) {
                            backup::sql::literal(output, "MYSQL", Value::Binary(bytes))?;
                        } else {
                            let value = std::str::from_utf8(bytes).context("备份文本不是 UTF-8")?;
                            let numeric = matches!(
                                kind,
                                "TINYINT"
                                    | "SMALLINT"
                                    | "MEDIUMINT"
                                    | "INT"
                                    | "BIGINT"
                                    | "TINYINT UNSIGNED"
                                    | "SMALLINT UNSIGNED"
                                    | "MEDIUMINT UNSIGNED"
                                    | "INT UNSIGNED"
                                    | "BIGINT UNSIGNED"
                                    | "FLOAT"
                                    | "DOUBLE"
                                    | "DECIMAL"
                            );
                            backup::sql::literal(
                                output,
                                "MYSQL",
                                if numeric {
                                    Value::Number(value)
                                } else {
                                    Value::Text(value)
                                },
                            )?;
                        }
                    }
                    output.write_all(b")")?;
                    Ok(())
                })?;
                count += 1;
                if count.is_multiple_of(256) {
                    control.records(256);
                }
                if count.is_multiple_of(1000) {
                    let name = format!("{uuid}.data.{page_number:05}.sql.gz");
                    definition_page.data(&writer.page(&name, page)?)?;
                    page_number += 1;
                    page = Page::new(directory)?;
                }
            }
            if !count.is_multiple_of(1000) {
                let name = format!("{uuid}.data.{page_number:05}.sql.gz");
                definition_page.data(&writer.page(&name, page)?)?;
            }
            control.records(count % 256);
        }
        let entry = definition_page.finish(&mut writer, &format!("{uuid}.meta.json.gz"))?;
        meta.objects.push(Object {
            uuid,
            kind: selected.kind.archive_name().into(),
            name: selected.table.name,
            rows: count.to_string(),
            metadata: entry,
        });
        records = records.saturating_add(count);
        control.object();
    }
    meta.end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    Ok((writer, meta, records))
}
