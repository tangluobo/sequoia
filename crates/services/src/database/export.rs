//! A dedicated connection streams all records independently of the UI page limit.
use super::{Databases, workbench};
use crate::data_export::{Control, Spool};
pub async fn query(
    connection: &Connection,
    database: &str,
    sql: &str,
    schema: Schema,
    parent: &Path,
    control: &Control,
) -> Result<Spool> {
    use sqlx::Executor;
    let statements =
        tomato_core::sql_script::split_for(connection.kind(), Some(sql)).collect::<Vec<_>>();
    ensure!(
        statements.len() == 1 && tomato_core::sql_script::is_select(Some(sql)),
        "只能重新读取单条查询语句，请导出当前结果"
    );
    let mut databases = Databases::default();
    let conn = tokio::select! {result=databases.session_in(connection,database)=>result?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    let readonly = match connection.kind() {
        "MYSQL" => Some("SET SESSION TRANSACTION READ ONLY"),
        "POSTGRESQL" => Some("BEGIN READ ONLY"),
        "SQLITE" => Some("PRAGMA query_only=ON"),
        "ORACLE" => Some("SET TRANSACTION READ ONLY"),
        "HIVE" => None,
        _ => anyhow::bail!("数据库类型不支持导出"),
    };
    if connection.kind() == "ORACLE" && !connection.text("__tomatoOracleOwner").is_empty() {
        let owner =
            crate::dialect::resolve("ORACLE")?.quote(connection.text("__tomatoOracleOwner"))?;
        let statement = format!("ALTER SESSION SET CURRENT_SCHEMA={owner}");
        tokio::select! {result=conn.execute(statement.as_str())=>{result?;},()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    }
    if let Some(readonly) = readonly {
        tokio::select! {result=conn.execute(readonly)=>{result?;},()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    }
    let projection = schema
        .columns
        .iter()
        .map(|c| workbench::projection(connection.kind(), c))
        .collect::<Result<Vec<_>>>()?;
    let wrapped = format!(
        "SELECT {} FROM ({}) tomato_export_source",
        projection.join(","),
        statements[0]
    );
    let mut spool = Spool::new(schema, parent)?;
    let _budget = conn
        .backend_mut::<super::oracle::Backend>()
        .map(|backend| backend.streaming());
    let mut stream = sqlx::query(&wrapped).fetch(conn);
    while let Some(row) = tokio::select! {result=stream.try_next()=>result?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")}
    {
        let cells = (0..spool.schema.columns.len())
            .map(|index| {
                if connection.kind() == "SQLITE"
                    || matches!(connection.kind(), "ORACLE" | "HIVE")
                        && spool.schema.columns[index].binary
                {
                    let cell = workbench::native_cell(&row, index)?;
                    if !spool.schema.columns[index].binary
                        && let Cell::Binary(bytes) = cell
                    {
                        return Ok(Cell::Text(
                            bytes.iter().map(|b| format!("{b:02x}")).collect(),
                        ));
                    }
                    Ok(cell)
                } else {
                    workbench::strings(&row, index)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        spool.push(&cells, control)?;
    }
    Ok(spool)
}
use anyhow::{Result, ensure};
use futures_util::TryStreamExt;
use std::path::Path;
use tomato_core::{
    Connection,
    data_export::Schema,
    workbench::{Cell, Sort, TableId},
};
pub async fn schema(
    connection: &Connection,
    database: &str,
    table: &TableId,
    control: &Control,
) -> Result<Schema> {
    let mut databases = Databases::default();
    let conn = tokio::select! {result=databases.session_in(connection,database)=>result?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    let definition = tokio::select! {result=workbench::describe(conn,connection.kind(),table)=>result?,()=control.wait_cancelled()=>anyhow::bail!("操作已取消")};
    Ok(Schema {
        table: definition.table,
        dialect: connection.kind().into(),
        columns: definition.columns,
    })
}
pub async fn table(
    connection: &Connection,
    database: &str,
    table: &TableId,
    sort: Option<&Sort>,
    criteria: &tomato_core::workbench::TableQuery,
    parent: &Path,
    control: &Control,
) -> Result<Spool> {
    let mut databases = Databases::default();
    let conn = tokio::select! { result=databases.session_in(connection,database)=>result?, ()=control.wait_cancelled()=>anyhow::bail!("操作已取消") };
    let definition = tokio::select! { result=workbench::describe(conn,connection.kind(),table)=>result?, ()=control.wait_cancelled()=>anyhow::bail!("操作已取消") };
    let projection = definition
        .columns
        .iter()
        .map(|column| workbench::projection(connection.kind(), column))
        .collect::<Result<Vec<_>>>()?;
    let mut sql = format!(
        "SELECT {} FROM {}",
        projection.join(","),
        workbench::plan::qualified(connection.kind(), table)?
    );
    let request = tomato_core::workbench::PageRequest {
        table: table.clone(),
        offset: 0,
        limit: 1000,
        sort: sort.cloned(),
        query: criteria.clone(),
    };
    let criteria = workbench::criteria::effective(&request);
    let (predicate, order) =
        workbench::criteria::clauses(connection.kind(), &definition, &criteria)?;
    sql.push_str(&predicate);
    sql.push_str(&order);
    let mut spool = Spool::new(
        Schema {
            table: table.clone(),
            dialect: connection.kind().into(),
            columns: definition.columns,
        },
        parent,
    )?;
    let _budget = conn
        .backend_mut::<super::oracle::Backend>()
        .map(|backend| backend.streaming());
    let mut stream = sqlx::query(&sql).fetch(conn);
    while let Some(row) = tokio::select! { result=stream.try_next()=>result?, ()=control.wait_cancelled()=>anyhow::bail!("操作已取消") }
    {
        let cells = spool
            .schema
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                if connection.kind() == "SQLITE"
                    || matches!(connection.kind(), "ORACLE" | "HIVE") && column.binary
                {
                    return workbench::native_cell(&row, index);
                }
                let mut value = workbench::strings(&row, index)?;
                if let Cell::Text(text) = &value {
                    ensure!(text.len() <= 16 * 1024 * 1024, "导出字段超过 16 MiB");
                    if column.binary {
                        value = Cell::Binary(workbench::unhex(text)?);
                    }
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        spool.push(&cells, control)?;
    }
    Ok(spool)
}
