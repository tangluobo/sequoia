//! Restore validates the entire container before changing any database object.
use crate::backup::{Archive, Control, Repository};
use anyhow::{Context, Result, ensure};
use sqlx::{AnyConnection, Executor};
use std::path::Path;
use tomato_core::{
    Connection,
    backup::{self, Definition, Metadata},
};
fn qualified(kind: &str, database: &str, name: &str) -> Result<String> {
    let dialect = crate::dialect::resolve(kind)?;
    Ok(format!(
        "{}.{}",
        dialect.quote(database)?,
        dialect.quote(name)?
    ))
}
pub(super) async fn mysql(
    conn: &mut AnyConnection,
    connection: &Connection,
    root: &Path,
    request: &backup::Restore,
    control: &Control,
) -> Result<(std::path::PathBuf, u64, u64)> {
    let path = Repository::new(root.into()).read_path(&request.scope, &request.name)?;
    let mut archive = Archive::open(&path)?;
    let metadata: Metadata = archive.json("meta.json", false)?;
    ensure!(
        metadata.database_type == connection.kind(),
        "备份数据库类型与当前连接不一致"
    );
    ensure!(metadata.encryption == "None", "此备份不是未加密 nb3 文件");
    ensure!(metadata.objects.len() <= 10_000, "备份对象超过 10,000 项");
    control.start(metadata.objects.len() as u64);
    control.log("校验备份文件");
    for object in &metadata.objects {
        control.check()?;
        archive.verify_with(&object.metadata, || control.check())?;
        let definition: Definition = archive.json(&object.metadata.filename, true)?;
        ensure!(
            matches!(
                definition.kind.as_str(),
                "Table" | "View" | "Function" | "Event"
            ) && definition.kind == object.kind
                && definition.name == object.name,
            "备份对象元数据不一致"
        );
        ensure!(
            !definition.ddl.is_empty(),
            "备份缺少对象定义，保留数据库原对象"
        );
        if definition.kind == "Table" {
            ensure!(definition.fields.is_some(), "备份缺少数据字段");
        }
        for data in definition.data.as_deref().unwrap_or_default() {
            archive.verify_with(data, || control.check())?;
            let mut rows = archive.records(&data.filename)?;
            while let Some(row) = rows.next_record()? {
                control.check()?;
                ensure!(
                    row.starts_with('(') && row.ends_with(')'),
                    "备份数据不是 SQL 值组"
                );
            }
        }
    }
    let row = sqlx::query("SELECT @@session.foreign_key_checks")
        .fetch_one(&mut *conn)
        .await?;
    let previous = super::super::workbench::text(&row, 0)?;
    ensure!(
        matches!(previous.as_str(), "0" | "1"),
        "无法读取外键检查状态"
    );
    conn.execute("SET FOREIGN_KEY_CHECKS=0").await?;
    let result: Result<(u64, u64)> = async {
        let mut records = 0u64;
        let dialect = crate::dialect::resolve("MYSQL")?;
        for object in &metadata.objects {
            control.check()?;
            let definition: Definition = archive.json(&object.metadata.filename, true)?;
            control.log(&format!("还原 {}: {}", definition.kind, definition.name));
            let kind = match definition.kind.as_str() {
                "Table" => "TABLE",
                "View" => "VIEW",
                "Function" => "FUNCTION",
                "Event" => "EVENT",
                _ => unreachable!(),
            };
            let target = qualified("MYSQL", &request.scope.database, &definition.name)?;
            conn.execute(format!("DROP {kind} IF EXISTS {target}").as_str())
                .await?;
            conn.execute(definition.ddl.as_str()).await?;
            if definition.kind == "Table" {
                let fields = definition
                    .fields
                    .as_deref()
                    .context("备份缺少字段")?
                    .iter()
                    .map(|field| dialect.quote(field))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                let prefix = format!("INSERT INTO {target} ({fields}) VALUES ");
                let mut sql = String::new();
                let mut pending = 0;
                for data in definition.data.as_deref().unwrap_or_default() {
                    let mut rows = archive.records(&data.filename)?;
                    while let Some(row) = rows.next_record()? {
                        control.check()?;
                        sql.clear();
                        sql.push_str(&prefix);
                        sql.push_str(row);
                        conn.execute(sql.as_str()).await?;
                        records = records.saturating_add(1);
                        pending += 1;
                        if pending == 256 {
                            control.records(pending);
                            pending = 0;
                        }
                    }
                }
                control.records(pending);
            }
            control.object();
        }
        Ok((metadata.objects.len() as u64, records))
    }
    .await;
    let cleanup = conn
        .execute(format!("SET FOREIGN_KEY_CHECKS={previous}").as_str())
        .await;
    let (objects, records) = result?;
    cleanup?;
    Ok((path, objects, records))
}
