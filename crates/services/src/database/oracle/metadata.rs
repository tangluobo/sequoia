//! Object-scoped metadata transforms; never changes the session transform defaults.
//! https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_METADATA.html
use super::super::workbench;
use anyhow::{Context, Result, ensure};
use sqlx::AnyConnection;
use tomato_core::workbench::{Cell, TableId};
const BUDGET: usize = 8 * 1024 * 1024;
fn copied_name(target: &TableId, name: &str) -> String {
    use sha1::{Digest as _, Sha1};
    let mut hash = Sha1::new();
    for value in [target.schema.as_str(), target.name.as_str(), name] {
        hash.update(value.as_bytes());
        hash.update([0]);
    }
    // Oracle 12.1's identifier limit is 30 bytes, including for CJK tables.
    format!("TMT_{:x}", hash.finalize())[..30].to_owned()
}
async fn identifier(session: &mut AnyConnection, sql: &str, values: &[Cell]) -> Result<Cell> {
    let rows = workbench::metadata(session, sql, values).await?;
    let value = workbench::text(rows.first().context("Oracle 未返回元数据句柄")?, 0)?;
    let number = value.parse::<i64>().context("Oracle 元数据句柄无效")?;
    ensure!(number > 0, "Oracle 元数据句柄无效");
    Ok(Cell::Integer(number))
}
pub(in crate::database) async fn ddl(
    session: &mut AnyConnection,
    object: &str,
    source: &TableId,
    target: &TableId,
    foreign_keys: bool,
) -> Result<String> {
    ensure!(
        matches!(
            object,
            "TABLE" | "VIEW" | "FUNCTION" | "INDEX" | "TRIGGER" | "REF_CONSTRAINT"
        ),
        "Oracle 对象类型无效"
    );
    let renames = if object == "TABLE" && source != target {
        let rows=workbench::metadata(session,"SELECT name FROM (SELECT constraint_name AS name,owner,table_name FROM all_constraints UNION SELECT index_name AS name,table_owner AS owner,table_name FROM all_indexes) WHERE owner=:1 AND table_name=:2 ORDER BY name",&[Cell::Text(source.schema.clone()),Cell::Text(source.name.clone())]).await?;
        ensure!(rows.len() <= 4096, "Oracle 附属对象超过4096项");
        rows.iter()
            .map(|row| workbench::text(row, 0))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|name| name != &source.name && name != &target.name)
            .map(|name| {
                let replacement = copied_name(target, &name);
                (name, replacement)
            })
            .collect()
    } else {
        Vec::new()
    };
    let handle = identifier(
        session,
        "SELECT DBMS_METADATA.OPEN(:1) FROM dual",
        &[Cell::Text(object.into())],
    )
    .await?;
    let result:Result<String>=async {
        workbench::query("BEGIN DBMS_METADATA.SET_FILTER(:1,'SCHEMA',:2); DBMS_METADATA.SET_FILTER(:1,'NAME',:3); END;",&[handle.clone(),Cell::Text(source.schema.clone()),Cell::Text(source.name.clone())]).execute(&mut *session).await?;
        let modify=identifier(session,"SELECT DBMS_METADATA.ADD_TRANSFORM(:1,'MODIFY') FROM dual",std::slice::from_ref(&handle)).await?;
        for (key, old, new) in [("REMAP_SCHEMA",&source.schema,&target.schema),("REMAP_NAME",&source.name,&target.name)] {
            if old != new {
                workbench::query("BEGIN DBMS_METADATA.SET_REMAP_PARAM(:1,:2,:3,:4); END;",&[modify.clone(),Cell::Text(key.into()),Cell::Text(old.clone()),Cell::Text(new.clone())]).execute(&mut *session).await?;
            }
        }
        for (old,new) in renames {
            workbench::query("BEGIN DBMS_METADATA.SET_REMAP_PARAM(:1,'REMAP_NAME',:2,:3); END;",&[modify.clone(),Cell::Text(old),Cell::Text(new)]).execute(&mut *session).await?;
        }
        let transform=identifier(session,"SELECT DBMS_METADATA.ADD_TRANSFORM(:1,'DDL') FROM dual",std::slice::from_ref(&handle)).await?;
        // Trigger DDL includes a separate ALTER TRIGGER for its enabled state.
        let terminators = if object == "TRIGGER" { "TRUE" } else { "FALSE" };
        let sql = format!("BEGIN DBMS_METADATA.SET_TRANSFORM_PARAM(:1,'SQLTERMINATOR',{terminators}); END;");
        workbench::query(&sql,std::slice::from_ref(&transform)).execute(&mut *session).await?;
        if matches!(object,"TABLE"|"INDEX") {
            workbench::query("BEGIN DBMS_METADATA.SET_TRANSFORM_PARAM(:1,'SEGMENT_ATTRIBUTES',FALSE); END;",std::slice::from_ref(&transform)).execute(&mut *session).await?;
        }
        if object=="TABLE" && !foreign_keys {
            workbench::query("BEGIN DBMS_METADATA.SET_TRANSFORM_PARAM(:1,'REF_CONSTRAINTS',FALSE); END;",&[transform]).execute(&mut *session).await?;
        }
        let rows=workbench::metadata(session,"SELECT DBMS_METADATA.FETCH_CLOB(:1) FROM dual",std::slice::from_ref(&handle)).await?;
        let sql=workbench::text(rows.first().context("Oracle 未返回建表语句")?,0)?;
        ensure!(!sql.trim().is_empty() && sql.len()<=BUDGET,"Oracle 建表语句为空或超过 8 MiB");
        Ok(sql)
    }.await;
    let close = workbench::query("BEGIN DBMS_METADATA.CLOSE(:1); END;", &[handle])
        .execute(&mut *session)
        .await;
    let result = result?;
    close?;
    Ok(result)
}
