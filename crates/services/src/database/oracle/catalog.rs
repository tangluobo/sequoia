//! Owner catalogs stay separate from the configured physical SID/service.
use super::super::workbench::{metadata, optional, text};
use super::*;
use tomato_core::workbench::{Cell, Column as TableColumn, TableDefinition, TableId};
mod columns;
mod foreign_keys;
mod objects;
pub(in crate::database) mod schema;
pub(in crate::database) use columns::read_columns;
pub(in crate::database) use foreign_keys::read as read_foreign_keys;
pub(in crate::database) use objects::read as read_objects;

pub(in crate::database) const TABLES: &str = "SELECT table_name,'BASE TABLE' FROM all_tables WHERE owner=:1 UNION ALL SELECT view_name,'VIEW' FROM all_views WHERE owner=:1 ORDER BY 1";
pub(in crate::database) fn binds(table: &TableId) -> [Cell; 2] {
    [
        Cell::Text(table.schema.clone()),
        Cell::Text(table.name.clone()),
    ]
}
pub(in crate::database) async fn describe(
    session: &mut AnyConnection,
    table: &TableId,
) -> anyhow::Result<TableDefinition> {
    let (rows, _) = read_columns(session, table).await?;
    let mut columns = Vec::with_capacity(rows.len());
    let mut comments = std::collections::BTreeMap::new();
    for row in rows {
        let name = row.name;
        let data_type = row.data_type;
        comments.insert(name.clone(), row.comment);
        columns.push(TableColumn {
            name,
            binary: matches!(data_type.as_str(), "RAW" | "LONG RAW" | "BLOB" | "BFILE"),
            data_type: data_type.clone(),
            nullable: row.nullable.eq_ignore_ascii_case("Y"),
            default: row.default.map(|v| v.trim().into()),
            primary_key: row.primary_key,
            generated: row.virtual_column.eq_ignore_ascii_case("YES")
                || row.identity.eq_ignore_ascii_case("ALWAYS"),
            auto_increment: !row.identity.is_empty(),
            type_schema: row.type_owner,
            type_name: data_type,
        });
    }
    let view = is_view(session, table).await?;
    Ok(TableDefinition {
        table: table.clone(),
        columns,
        comments,
        view,
        transactional: true,
    })
}
pub(in crate::database) async fn is_view(
    session: &mut AnyConnection,
    table: &TableId,
) -> anyhow::Result<bool> {
    Ok(metadata(session,"SELECT object_type FROM all_objects WHERE owner=:1 AND object_name=:2 AND object_type IN ('TABLE','VIEW','MATERIALIZED VIEW')",&binds(table)).await?
        .iter().any(|r|text(r,0).is_ok_and(|t|t!="TABLE")))
}
