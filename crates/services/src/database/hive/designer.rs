//! Hive structure is read through its native catalog, then saved by the shared designer.
use super::*;
use tomato_core::{
    table_design::{
        Column, Create, Options,
        alter::{InfoTable, Snapshot},
    },
    workbench::TableId,
};
pub(in crate::database) async fn read(
    session: &mut AnyConnection,
    table: &TableId,
) -> Result<Snapshot> {
    read_mode(session, table, true).await
}
pub(in crate::database) async fn read_mode(
    session: &mut AnyConnection,
    table: &TableId,
    include_sql: bool,
) -> Result<Snapshot> {
    let (described, properties) = super::catalog::details(session, table).await?;
    let sql = if include_sql {
        super::catalog::ddl(session, table).await?
    } else {
        String::new()
    };
    let mut columns = Vec::with_capacity(described.columns.len());
    let mut raw_columns = Vec::with_capacity(described.columns.len());
    for field in &described.columns {
        let (data_type, length, scale) = split_type(&field.data_type);
        raw_columns.push(serde_json::to_string(&(&field.name, &field.data_type))?);
        columns.push(Column {
            name: field.name.clone(),
            data_type,
            length,
            scale,
            comment: described
                .comments
                .get(&field.name)
                .cloned()
                .unwrap_or_default(),
            ..Default::default()
        });
    }
    Ok(Snapshot {
        definition: Create {
            auxiliary: Vec::new(),
            table: table.clone(),
            columns,
            indexes: Vec::new(),
            options: Options {
                engine: properties
                    .get("table type")
                    .cloned()
                    .unwrap_or_else(|| "Hive".into()),
                ..Default::default()
            },
            comment: properties.get("comment").cloned().unwrap_or_default(),
        },
        extra_options: [
            String::new(),
            properties.get("inputformat").cloned().unwrap_or_default(),
            String::new(),
        ],
        backslash_escapes: false,
        raw_columns,
        indexes: InfoTable::default(),
        foreign_keys: InfoTable::default(),
        triggers: InfoTable::default(),
        sql,
        view: described.view,
        primary_name: None,
    })
}
fn split_type(value: &str) -> (String, String, String) {
    if !value.contains('<')
        && let Some((base, rest)) = value.split_once('(')
        && let Some(size) = rest.strip_suffix(')')
    {
        let (length, scale) = size.split_once(',').unwrap_or((size, ""));
        return (
            base.to_ascii_uppercase(),
            length.trim().into(),
            scale.trim().into(),
        );
    }
    (
        if value.contains('<') {
            value.into()
        } else {
            value.to_ascii_uppercase()
        },
        String::new(),
        String::new(),
    )
}
