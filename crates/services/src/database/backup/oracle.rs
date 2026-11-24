//! Native Oracle nb3: bounded metadata, parameter-compatible SQL pages and phased restore.
use super::super::{oracle::metadata, workbench};
use crate::backup::{Control, DefinitionPage, Page, Writer};
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt as _;
use sqlx::{AnyConnection, Executor as _};
use std::path::Path;
use tomato_core::{
    backup::{self, Definition, Kind, Metadata, Object, OracleDefinition, Selection},
    workbench::{Cell, Column, TableId},
};
mod restore;
#[cfg(test)]
mod tests;
pub(super) use restore::restore;
fn qualified(table: &TableId) -> Result<String> {
    workbench::plan::qualified("ORACLE", table)
}
fn quote(value: &str) -> Result<String> {
    crate::dialect::resolve("ORACLE")?.quote(value)
}
fn literal(value: &str) -> Result<String> {
    let mut bytes = Vec::new();
    backup::sql::literal(&mut bytes, "ORACLE", backup::sql::Value::Text(value))?;
    Ok(String::from_utf8(bytes)?)
}
pub(super) async fn list(conn: &mut AnyConnection, owner: &str) -> Result<Vec<Selection>> {
    let mut result = Vec::new();
    let mut retained = 0usize;
    for (kind, sql) in [
        (
            Kind::Table,
            "SELECT table_name FROM all_tables WHERE owner=:1 ORDER BY table_name",
        ),
        (
            Kind::View,
            "SELECT view_name FROM all_views WHERE owner=:1 ORDER BY view_name",
        ),
        (
            Kind::Function,
            "SELECT object_name FROM all_objects WHERE owner=:1 AND object_type='FUNCTION' ORDER BY object_name",
        ),
    ] {
        for row in workbench::metadata(conn, sql, &[Cell::Text(owner.into())]).await? {
            let name = workbench::text(&row, 0)?;
            retained += name.capacity() + owner.len() + std::mem::size_of::<Selection>();
            ensure!(
                result.len() < 10_000 && retained <= 8 * 1024 * 1024,
                "备份对象超过 10000 项或 8 MiB"
            );
            result.push(Selection {
                kind,
                table: TableId {
                    schema: owner.into(),
                    name,
                },
            });
        }
    }
    Ok(result)
}
async fn dependent(
    conn: &mut AnyConnection,
    sql: &str,
    object: &str,
    table: &TableId,
) -> Result<Vec<String>> {
    let names = workbench::metadata(
        conn,
        sql,
        &[
            Cell::Text(table.schema.clone()),
            Cell::Text(table.name.clone()),
        ],
    )
    .await?;
    let mut result = Vec::new();
    let mut retained = 0usize;
    for row in names {
        let name = workbench::text(&row, 0)?;
        let id = TableId {
            schema: table.schema.clone(),
            name,
        };
        let ddl = metadata::ddl(conn, object, &id, &id, true).await?;
        retained += ddl.capacity();
        ensure!(retained <= 8 * 1024 * 1024, "Oracle 附属对象超过 8 MiB");
        result.push(ddl);
    }
    Ok(result)
}
async fn definition(
    conn: &mut AnyConnection,
    selection: &Selection,
) -> Result<(Definition, Vec<Column>)> {
    let object = match selection.kind {
        Kind::Table => "TABLE",
        Kind::View => "VIEW",
        Kind::Function => "FUNCTION",
        Kind::Event => anyhow::bail!("Oracle 不提供 MySQL Event 对象"),
    };
    let mut definition = Definition {
        meta_version: "30100".into(),
        name: selection.table.name.clone(),
        kind: selection.kind.archive_name().into(),
        ddl: metadata::ddl(conn, object, &selection.table, &selection.table, false).await?,
        sub_ddl: Some(vec![]),
        auto_increment: None,
        fields: None,
        trigger_ddl: Some(vec![]),
        index_ddl: Some(vec![]),
        data: None,
        postgres: None,
        oracle: Some(OracleDefinition {
            schema: selection.table.schema.clone(),
            standard_strings: true,
        }),
    };
    let mut columns = Vec::new();
    if selection.kind == Kind::Table {
        let table = workbench::describe(conn, "ORACLE", &selection.table).await?;
        // Virtual expressions are retained in CREATE DDL. Identity values are
        // backed up and restored with a temporary BY DEFAULT mode.
        columns = table
            .columns
            .into_iter()
            .filter(|column| !column.generated || column.auto_increment)
            .collect();
        definition.fields = Some(columns.iter().map(|c| c.name.clone()).collect());
        let mut after = Vec::new();
        if let Some(row) = workbench::metadata(
            conn,
            "SELECT comments FROM all_tab_comments WHERE owner=:1 AND table_name=:2",
            &[
                Cell::Text(selection.table.schema.clone()),
                Cell::Text(selection.table.name.clone()),
            ],
        )
        .await?
        .first()
            && let Some(comment) = workbench::optional(row, 0)?
        {
            after.push(format!(
                "COMMENT ON TABLE {} IS {}",
                qualified(&selection.table)?,
                literal(&comment)?
            ));
        }
        for (name, comment) in table.comments {
            if !comment.is_empty() {
                after.push(format!(
                    "COMMENT ON COLUMN {}.{} IS {}",
                    qualified(&selection.table)?,
                    quote(&name)?,
                    literal(&comment)?
                ));
            }
        }
        after.extend(dependent(conn,"SELECT constraint_name FROM all_constraints WHERE owner=:1 AND table_name=:2 AND constraint_type='R' ORDER BY constraint_name","REF_CONSTRAINT",&selection.table).await?);
        definition.sub_ddl = Some(after);
        definition.index_ddl=Some(dependent(conn,"SELECT i.index_name FROM all_indexes i WHERE i.owner=:1 AND i.table_name=:2 AND NOT EXISTS (SELECT 1 FROM all_constraints c WHERE c.owner=i.owner AND c.index_name=i.index_name AND c.constraint_type IN ('P','U')) ORDER BY i.index_name","INDEX",&selection.table).await?);
        definition.trigger_ddl=Some(dependent(conn,"SELECT trigger_name FROM all_triggers WHERE owner=:1 AND table_name=:2 ORDER BY trigger_name","TRIGGER",&selection.table).await?);
    }
    ensure!(
        serde_json::to_vec(&definition)?.len() <= 8 * 1024 * 1024,
        "Oracle 对象定义超过 8 MiB"
    );
    Ok((definition, columns))
}
fn write_cell(output: &mut dyn std::io::Write, cell: &Cell, column: &Column) -> Result<()> {
    use backup::sql::Value;
    match cell {
        Cell::Null => backup::sql::literal(output, "ORACLE", Value::Null),
        Cell::Binary(bytes) => backup::sql::literal(output, "ORACLE", Value::Binary(bytes)),
        Cell::Integer(n) => backup::sql::literal(output, "ORACLE", Value::Number(&n.to_string())),
        Cell::Real(n) => {
            ensure!(n.is_finite(), "Oracle 备份暂不接受非有限浮点值");
            backup::sql::literal(output, "ORACLE", Value::Number(&n.to_string()))
        }
        Cell::Boolean(n) => backup::sql::literal(output, "ORACLE", Value::Boolean(*n)),
        Cell::Text(value) => backup::sql::literal(
            output,
            "ORACLE",
            if ["NUMBER", "FLOAT", "BINARY_FLOAT", "BINARY_DOUBLE"]
                .contains(&column.data_type.as_str())
                && backup::sql::is_number(value)
            {
                Value::Number(value)
            } else {
                Value::Text(value)
            },
        ),
    }
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
        database_type: "ORACLE".into(),
        catalog: String::new(),
        schema: request.scope.database.clone(),
        start_time: now.clone(),
        end_time: now,
        encryption: "None".into(),
        comment: request.comment.clone(),
        objects: vec![],
    };
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    control.start(request.objects.len() as u64);
    for selected in &request.objects {
        control.check()?;
        ensure!(
            seen.insert((selected.kind.archive_name(), &selected.table.name)),
            "备份对象重复"
        );
        let mut selected = selected.clone();
        if selected.table.schema.is_empty() {
            selected.table.schema = request.scope.database.clone();
        }
        ensure!(
            selected.table.schema == request.scope.database,
            "备份对象不属于选中的 Oracle 用户"
        );
        control.log(&format!(
            "处理 {}: {}",
            selected.kind.label(),
            selected.table.name
        ));
        let (definition, columns) = definition(conn, &selected).await?;
        let mut page_definition = DefinitionPage::new(directory, &definition)?;
        let uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let mut count = 0u64;
        if selected.kind == Kind::Table {
            ensure!(!columns.is_empty(), "表没有可备份的普通字段");
            let names = columns
                .iter()
                .map(|c| quote(&c.name))
                .collect::<Result<Vec<_>>>()?
                .join(",");
            let sql = format!("SELECT {names} FROM {}", qualified(&selected.table)?);
            let _budget = conn
                .backend_mut::<super::super::oracle::Backend>()
                .map(|backend| backend.streaming());
            let mut stream = sqlx::query(&sql).fetch(&mut *conn);
            let mut page = Page::new(directory)?;
            let mut number = 0u64;
            while let Some(row) = tokio::select! {row=stream.try_next()=>row?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消") }
            {
                control.check()?;
                page.record(|output| {
                    output.write_all(b"(")?;
                    for (index, column) in columns.iter().enumerate() {
                        if index > 0 {
                            output.write_all(b", ")?;
                        }
                        write_cell(output, &workbench::native_cell(&row, index)?, column)?;
                    }
                    output.write_all(b")")?;
                    Ok(())
                })?;
                count += 1;
                if count.is_multiple_of(256) {
                    control.records(256);
                }
                if count.is_multiple_of(1000) {
                    page_definition
                        .data(&writer.page(&format!("{uuid}.data.{number:05}.sql.gz"), page)?)?;
                    number += 1;
                    page = Page::new(directory)?;
                }
            }
            if !count.is_multiple_of(1000) {
                page_definition
                    .data(&writer.page(&format!("{uuid}.data.{number:05}.sql.gz"), page)?)?;
            }
            control.records(count % 256);
        }
        let entry = page_definition.finish(&mut writer, &format!("{uuid}.meta.json.gz"))?;
        metadata.objects.push(Object {
            uuid,
            kind: selected.kind.archive_name().into(),
            name: selected.table.name,
            rows: count.to_string(),
            metadata: entry,
        });
        total += count;
        control.object();
    }
    metadata.end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    Ok((writer, metadata, total))
}
