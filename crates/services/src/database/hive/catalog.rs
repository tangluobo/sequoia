//! Hive catalogs use the same SHOW/DESCRIBE statements as the Java handler.
use super::super::workbench::{metadata, text};
use super::*;
use anyhow::ensure;
use std::collections::{BTreeMap, BTreeSet};
use tomato_core::{
    database_objects::{Catalog, Object},
    workbench::{Column, TableDefinition, TableId, TableInfo},
};
fn qualified(table: &TableId) -> Result<String> {
    super::super::workbench::plan::qualified("HIVE", table)
}
pub(in crate::database) async fn completion(
    session: &mut AnyConnection,
    schema: &str,
) -> Result<tomato_core::sql_completion::Metadata> {
    use tomato_core::{
        ai_sql::editor::SchemaTable,
        sql_completion::{Metadata, Table},
    };
    let databases = super::super::workbench::catalog::databases(session, "HIVE").await?;
    let mut output = Metadata {
        databases,
        schema: schema.into(),
        database: schema.into(),
        namespaces: vec![schema.into()],
        ..Default::default()
    };
    let mut retained = 0usize;
    let mut fields = 0usize;
    for info in tables(session, schema).await? {
        let definition = describe(session, &info.table).await?;
        let mut columns = Vec::with_capacity(definition.columns.len());
        let mut ai = SchemaTable {
            text: format!("TABLE {}", info.table.name),
            terms: vec![info.table.name.clone()],
            order: output.tables.len(),
        };
        for column in definition.columns {
            fields += 1;
            ensure!(fields <= 10_000, "补全字段超过 10,000 项，请缩小数据库范围");
            ai.text
                .push_str(&format!("\n  {} {}", column.name, column.data_type));
            if let Some(comment) = definition
                .comments
                .get(&column.name)
                .filter(|s| !s.is_empty())
            {
                ai.text.push_str(" -- ");
                ai.text.extend(
                    comment
                        .chars()
                        .map(|ch| if ch.is_control() { ' ' } else { ch }),
                );
                ai.terms.push(comment.clone());
            }
            ai.terms.push(column.name.clone());
            columns.push(column.name);
        }
        retained += ai.text.capacity()
            + ai.terms.iter().map(String::capacity).sum::<usize>()
            + columns.iter().map(String::capacity).sum::<usize>()
            + 256;
        ensure!(
            retained <= 8 * 1024 * 1024,
            "补全元数据超过 8 MiB，请缩小数据库范围"
        );
        output.tables.push(Table {
            name: info.table.name,
            columns,
        });
        output.ai_tables.push(ai);
    }
    Ok(output)
}
fn name_index(row: &AnyRow) -> usize {
    use sqlx::Column as _;
    row.columns
        .iter()
        .position(|column| {
            matches!(
                column.name().to_ascii_lowercase().as_str(),
                "tablename" | "viewname" | "tab_name"
            )
        })
        .unwrap_or(0)
}
pub(in crate::database) async fn tables(
    session: &mut AnyConnection,
    schema: &str,
) -> Result<Vec<TableInfo>> {
    let namespace = if schema.is_empty() {
        String::new()
    } else {
        format!(" IN {}", crate::dialect::resolve("HIVE")?.quote(schema)?)
    };
    let mut tables = BTreeMap::new();
    for (sql, view) in [
        (format!("SHOW TABLES{namespace}"), false),
        (format!("SHOW VIEWS{namespace}"), true),
    ] {
        for row in metadata(session, &sql, &[]).await? {
            let name = text(&row, name_index(&row))?;
            ensure!(
                tables.contains_key(&name) || tables.len() < 10_000,
                "Hive 表和视图超过 10000 项"
            );
            tables.insert(name, view);
        }
    }
    Ok(tables
        .into_iter()
        .map(|(name, view)| TableInfo {
            table: TableId {
                schema: schema.into(),
                name,
            },
            view,
        })
        .collect())
}
pub(in crate::database) async fn describe(
    session: &mut AnyConnection,
    table: &TableId,
) -> Result<TableDefinition> {
    Ok(details(session, table).await?.0)
}
pub(in crate::database) async fn details(
    session: &mut AnyConnection,
    table: &TableId,
) -> Result<(TableDefinition, BTreeMap<String, String>)> {
    let rows = metadata(
        session,
        &format!("DESCRIBE FORMATTED {}", qualified(table)?),
        &[],
    )
    .await?;
    let mut columns = Vec::new();
    let mut comments = BTreeMap::new();
    let mut properties = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut fields = true;
    for row in rows {
        let get = |index: usize| -> Result<String> {
            if index < row.columns.len() {
                text(&row, index)
            } else {
                Ok(String::new())
            }
        };
        let name = get(0)?.trim().to_owned();
        let kind = get(1)?.trim().to_owned();
        let comment = get(2)?;
        if name.starts_with('#') {
            if !name.to_ascii_lowercase().contains("partition information")
                && !name.to_ascii_lowercase().contains("col_name")
            {
                fields = false;
            }
            continue;
        }
        if fields {
            if name.is_empty()
                || kind.is_empty()
                || (name == "col_name" && kind == "data_type")
                || !seen.insert(name.clone())
            {
                continue;
            }
            ensure!(columns.len() < 4096, "Hive 表字段超过 4096 项");
            comments.insert(name.clone(), comment);
            columns.push(Column {
                name,
                data_type: kind.clone(),
                nullable: true,
                default: None,
                primary_key: 0,
                generated: false,
                auto_increment: false,
                binary: kind.eq_ignore_ascii_case("binary"),
                type_schema: String::new(),
                type_name: kind,
            });
        } else {
            let (key, value) = if name.is_empty() {
                (kind, comment.trim().to_owned())
            } else {
                (name, kind)
            };
            if !key.is_empty() {
                properties.insert(key.trim_end_matches(':').trim().to_ascii_lowercase(), value);
            }
        }
    }
    ensure!(!columns.is_empty(), "Hive 表不存在、没有字段或无权读取结构");
    let view = properties
        .get("table type")
        .is_some_and(|value| value.to_ascii_uppercase().contains("VIEW"));
    let transactional = properties
        .get("transactional")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    Ok((
        TableDefinition {
            table: table.clone(),
            columns,
            comments,
            view,
            transactional,
        },
        properties,
    ))
}
pub(in crate::database) async fn objects(
    session: &mut AnyConnection,
    schema: &str,
) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    let mut retained = 0usize;
    for info in tables(session, schema).await? {
        let (_, props) = details(session, &info.table).await?;
        let engine = props
            .get("table type")
            .cloned()
            .unwrap_or_else(|| "Hive".into());
        let comment = props.get("comment").cloned().unwrap_or_default();
        let data_length = props
            .get("totalsize")
            .and_then(|s| s.parse::<u64>().ok())
            .map(tomato_core::database_objects::data_length)
            .unwrap_or_default();
        let rows = props.get("numrows").cloned().unwrap_or_default();
        let update_time = props
            .get("last_modified_time")
            .or_else(|| props.get("transient_lastddltime"))
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|stamp| chrono::DateTime::from_timestamp(stamp, 0))
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        retained = retained.saturating_add(
            info.table.name.len() + comment.len() + engine.len() + rows.len() + 256,
        );
        ensure!(retained <= 8 * 1024 * 1024, "Hive 对象目录超过 8 MiB");
        catalog.objects.push(Object {
            table: info.table,
            view: info.view,
            comment,
            engine,
            auto_increment: String::new(),
            update_time,
            data_length,
            rows,
            fields: Vec::new(),
        });
    }
    Ok(catalog)
}
pub(in crate::database) async fn ddl(
    session: &mut AnyConnection,
    table: &TableId,
) -> Result<String> {
    let rows = metadata(
        session,
        &format!("SHOW CREATE TABLE {}", qualified(table)?),
        &[],
    )
    .await?;
    ensure!(!rows.is_empty(), "Hive 未返回建表语句");
    rows.iter()
        .map(|row| text(row, 0))
        .collect::<Result<Vec<_>>>()
        .map(|rows| rows.join("\n"))
}
