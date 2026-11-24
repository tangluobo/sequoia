//! PostgreSQL nb3 adapter. Catalog SQL stays separate from streaming rows and restore phases.
use super::super::workbench;
use crate::backup::{Control, DefinitionPage, Page, Writer};
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt;
use sqlx::{AnyConnection, Executor as _, Row};
use std::path::Path;
use tomato_core::{
    backup::{self, Definition, Kind, Metadata, Object, PostgresDefinition, Selection},
    workbench::{Cell, TableId},
};
mod catalog;
mod dependencies;
mod restore;
mod table;
mod types;
pub(super) use catalog::list;
pub(super) use restore::restore;
fn quote(name: &str) -> Result<String> {
    crate::dialect::resolve("POSTGRESQL")?.quote(name)
}
pub(super) fn qualified(table: &TableId) -> Result<String> {
    Ok(format!("{}.{}", quote(&table.schema)?, quote(&table.name)?))
}
fn literal(value: &str) -> Result<String> {
    let mut bytes = Vec::new();
    backup::sql::literal(&mut bytes, "POSTGRESQL", backup::sql::Value::Text(value))?;
    Ok(String::from_utf8(bytes)?)
}
fn display(selection: &Selection) -> String {
    format!("{}.{}", selection.table.schema, selection.table.name)
}
async fn rows(
    conn: &mut AnyConnection,
    sql: &str,
    table: &TableId,
) -> Result<Vec<sqlx::any::AnyRow>> {
    workbench::metadata(
        conn,
        sql,
        &[
            Cell::Text(table.schema.clone()),
            Cell::Text(table.name.clone()),
        ],
    )
    .await
}
pub(super) async fn objects(
    conn: &mut AnyConnection,
    request: &backup::Create,
    directory: &Path,
    control: &Control,
) -> Result<(Writer, Metadata, u64)> {
    let mut writer = Writer::new(directory)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let mut metadata = Metadata {
        meta_version: "30100".into(),
        database_type: "POSTGRESQL".into(),
        catalog: request.scope.database.clone(),
        schema: String::new(),
        start_time: now.clone(),
        end_time: now,
        encryption: "None".into(),
        comment: request.comment.clone(),
        objects: Vec::new(),
    };
    let mut count = 0u64;
    let mut seen = std::collections::HashSet::new();
    control.start(request.objects.len() as u64);
    for selected in &request.objects {
        control.check()?;
        ensure!(
            seen.insert((
                selected.kind.archive_name(),
                &selected.table.schema,
                &selected.table.name
            )),
            "备份对象重复"
        );
        control.log(&format!(
            "处理 {}: {}",
            selected.kind.label(),
            display(selected)
        ));
        let definition = catalog::definition(conn, selected).await?;
        let fields = definition.fields.as_deref().unwrap_or_default();
        let mut page_definition = DefinitionPage::new(directory, &definition)?;
        let uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let mut records = 0u64;
        if selected.kind == Kind::Table {
            let columns = if fields.is_empty() {
                "1::text".to_owned()
            } else {
                fields
                    .iter()
                    .map(|field| Ok(format!("CAST({} AS text)", quote(field)?)))
                    .collect::<Result<Vec<_>>>()?
                    .join(",")
            };
            let sql = format!("SELECT {columns} FROM ONLY {}", qualified(&selected.table)?);
            conn.execute("SET LOCAL statement_timeout=0").await?;
            let native = conn
                .backend_mut::<sqlx::PgConnection>()
                .context("PostgreSQL 会话类型不匹配")?;
            let mut stream = sqlx::query(&sql).fetch(native);
            let mut page = Page::new(directory)?;
            let mut number = 0u64;
            while let Some(row) = tokio::select! {value=stream.try_next()=>value?,()=control.wait_cancelled()=>{anyhow::bail!("操作已取消")}}
            {
                control.check()?;
                page.record(|output| {
                    output.write_all(b"(")?;
                    for index in 0..fields.len() {
                        if index > 0 {
                            output.write_all(b", ")?;
                        }
                        let value = row.try_get::<Option<&str>, _>(index)?;
                        backup::sql::literal(
                            output,
                            "POSTGRESQL",
                            value.map_or(backup::sql::Value::Null, backup::sql::Value::Text),
                        )?;
                    }
                    output.write_all(b")")?;
                    Ok(())
                })?;
                records += 1;
                if records.is_multiple_of(256) {
                    control.records(256);
                }
                if records.is_multiple_of(1000) {
                    page_definition
                        .data(&writer.page(&format!("{uuid}.data.{number:05}.sql.gz"), page)?)?;
                    number += 1;
                    page = Page::new(directory)?;
                }
            }
            if !records.is_multiple_of(1000) {
                page_definition
                    .data(&writer.page(&format!("{uuid}.data.{number:05}.sql.gz"), page)?)?;
            }
            control.records(records % 256);
        }
        let entry = page_definition.finish(&mut writer, &format!("{uuid}.meta.json.gz"))?;
        metadata.objects.push(Object {
            uuid,
            kind: selected.kind.archive_name().into(),
            name: definition.name,
            rows: records.to_string(),
            metadata: entry,
        });
        count = count.saturating_add(records);
        control.object();
    }
    metadata.end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    Ok((writer, metadata, count))
}
