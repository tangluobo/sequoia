use super::*;
use crate::backup::{Archive, Repository};
use sqlx::Connection as _;
use std::sync::atomic::{AtomicBool, Ordering};
fn standard(definition: &Definition) -> bool {
    definition
        .oracle
        .as_ref()
        .is_some_and(|o| o.standard_strings)
}
async fn execute(conn: &mut AnyConnection, sql: &str) -> Result<()> {
    let statements = tomato_core::sql_script::split_for("ORACLE", Some(sql));
    for (count, statement) in statements.enumerate() {
        ensure!(count < 4096, "Oracle 对象包含过多语句");
        conn.execute(statement).await?;
    }
    Ok(())
}
/// Earlier Tomato archives contain Oracle's unterminated CREATE + ALTER form.
/// Match the trailing state statement against the archived trigger's own name.
fn legacy_trigger_parts(sql: &str) -> Option<(&str, &str)> {
    let (create, alter) = sql.trim_end().rsplit_once('\n')?;
    let alter = alter.trim();
    let (target, state) = alter.strip_prefix("ALTER TRIGGER ")?.rsplit_once(' ')?;
    let target = target.trim();
    let header = create.trim_start().strip_prefix("CREATE OR REPLACE ")?;
    let header = header
        .strip_prefix("NONEDITIONABLE ")
        .or_else(|| header.strip_prefix("EDITIONABLE "))
        .unwrap_or(header)
        .strip_prefix("TRIGGER ")?;
    let rest = header.strip_prefix(target)?;
    (matches!(state, "ENABLE" | "DISABLE")
        && rest.starts_with(char::is_whitespace)
        && create.trim_end().ends_with(';'))
    .then_some((create, alter))
}
async fn writable_columns(
    conn: &mut AnyConnection,
    table: &TableId,
) -> Result<tomato_core::workbench::TableDefinition> {
    // Restoring explicit values does not use defaults, comments or keys.
    // In particular DATA_DEFAULT is LONG and uses its own fetch protocol.
    let columns = workbench::metadata(
        conn,
        "SELECT column_name,data_type,virtual_column FROM all_tab_cols WHERE owner=:1 AND table_name=:2 AND hidden_column='NO' ORDER BY column_id",
        &super::super::super::oracle::catalog::binds(table),
    )
    .await?;
    ensure!(
        !columns.is_empty() && columns.len() <= 4096,
        "表不存在或可见字段数量无效"
    );
    let columns = columns
        .iter()
        .map(|row| {
            Ok(Column {
                name: workbench::text(row, 0)?,
                data_type: workbench::text(row, 1)?,
                generated: workbench::text(row, 2)? == "YES",
                nullable: true,
                default: None,
                primary_key: 0,
                auto_increment: false,
                binary: false,
                type_schema: String::new(),
                type_name: String::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(tomato_core::workbench::TableDefinition {
        table: table.clone(),
        columns,
        comments: Default::default(),
        view: false,
        transactional: true,
    })
}
fn object_type(definition: &Definition) -> Result<&'static str> {
    Ok(match definition.kind.as_str() {
        "Table" => "TABLE",
        "View" => "VIEW",
        "Function" => "FUNCTION",
        _ => anyhow::bail!("Oracle 备份对象类型无效"),
    })
}
fn table(definition: &Definition, metadata: &Metadata) -> TableId {
    TableId {
        schema: definition
            .oracle
            .as_ref()
            .map_or_else(|| metadata.schema.clone(), |o| o.schema.clone()),
        name: definition.name.clone(),
    }
}
async fn restore_table(
    conn: &mut AnyConnection,
    archive: &mut Archive,
    definition: &Definition,
    metadata: &Metadata,
    control: &Control,
    committing: &AtomicBool,
    records: &mut u64,
) -> Result<()> {
    let table = table(definition, metadata);
    let identity = crate::database::oracle::identity::Identity::prepare(conn, &table)
        .await
        .with_context(|| format!("准备表 {} 的 Oracle 标识列", table.name))?;
    let before = *records;
    let outcome: Result<()> = async {
        let described = writable_columns(conn, &table)
            .await
            .with_context(|| format!("读取待还原表 {} 的字段结构", table.name))?;
        let fields = definition.fields.as_deref().context("缺少备份字段")?;
        let positions = fields
            .iter()
            .map(|name| {
                described
                    .columns
                    .iter()
                    .position(|c| c.name == *name && !c.generated)
                    .with_context(|| format!("备份字段 {name} 在新表中不存在或不可写"))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut transaction = conn.begin().await?;
        let mut pending = 0u64;
        for data in definition.data.as_deref().unwrap_or_default() {
            let mut rows = archive.records_with_strings(&data.filename, standard(definition))?;
            while let Some(row) = rows.next_record()? {
                control.check()?;
                let cells =
                    super::super::oracle_values::parse(row, standard(definition), fields.len())?;
                let mut values = vec![None; described.columns.len()];
                for (position, mut value) in positions.iter().zip(cells) {
                    let column = &described.columns[*position];
                    if let Cell::Text(text) = &mut value
                        && (column.data_type == "DATE" || column.data_type.starts_with("TIMESTAMP"))
                    {
                        if text.len() == 10 {
                            text.push_str(" 00:00:00.0");
                        } else if text.len() == 19 {
                            text.push_str(".0");
                        }
                    }
                    values[*position] = Some(value);
                }
                let statement = workbench::plan::build(
                    "ORACLE",
                    &described,
                    &tomato_core::workbench::RowChange::Insert { values },
                )?
                .context("备份记录没有可写字段")?;
                let affected = workbench::query(&statement.sql, &statement.params)
                    .execute(&mut *transaction)
                    .await
                    .with_context(|| {
                        format!(
                            "还原表 {} 的第 {} 行失败",
                            table.name,
                            *records - before + pending + 1
                        )
                    })?
                    .rows_affected();
                ensure!(affected == 1, "还原记录影响行数不是1");
                pending += 1;
                if pending == 256 {
                    committing.store(true, Ordering::Release);
                    let result = transaction.commit().await;
                    committing.store(false, Ordering::Release);
                    result.with_context(|| format!("提交表 {} 的当批还原数据", table.name))?;
                    *records += pending;
                    control.records(pending);
                    pending = 0;
                    transaction = conn.begin().await?;
                }
            }
        }
        committing.store(true, Ordering::Release);
        let result = transaction.commit().await;
        committing.store(false, Ordering::Release);
        result.with_context(|| format!("提交表 {} 的最后一批还原数据", table.name))?;
        *records += pending;
        control.records(pending);
        Ok(())
    }
    .await;
    // A failed row rolls back its open batch before restoring ALWAYS. Earlier
    // committed batches retain their explicit IDs and advance the sequence.
    let cleanup = identity.finish(conn, *records > before).await;
    match (outcome, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("恢复 Oracle 标识列失败：{cleanup}")))
        }
    }
}
pub(in crate::database::backup) async fn restore(
    conn: &mut AnyConnection,
    root: &Path,
    request: &backup::Restore,
    control: &Control,
    committing: &AtomicBool,
) -> Result<(std::path::PathBuf, u64, u64)> {
    let path = Repository::new(root.into()).read_path(&request.scope, &request.name)?;
    let mut archive = Archive::open(&path)?;
    let metadata: Metadata = archive.json("meta.json", false)?;
    ensure!(
        metadata.database_type == "ORACLE" && metadata.encryption == "None",
        "备份类型或加密格式不匹配"
    );
    ensure!(
        !metadata.objects.is_empty() && metadata.objects.len() <= 10000,
        "备份对象数量无效"
    );
    let owner = request
        .namespace
        .as_deref()
        .unwrap_or(&request.scope.database);
    ensure!(
        metadata.schema == owner,
        "Oracle nb3 按原用户模式还原，请在备份所属用户下还原"
    );
    quote(owner)?;
    control.start(metadata.objects.len() as u64);
    control.log("校验备份文件");
    let mut keys = std::collections::HashSet::new();
    let mut tables = std::collections::HashSet::new();
    for object in &metadata.objects {
        control.check()?;
        archive.verify_with(&object.metadata, || control.check())?;
        let definition: Definition = archive.json(&object.metadata.filename, true)?;
        ensure!(
            definition.name == object.name
                && definition.kind == object.kind
                && !definition.ddl.trim().is_empty()
                && keys.insert((definition.kind.clone(), definition.name.clone())),
            "备份对象缺失、重复或定义不一致"
        );
        object_type(&definition)?;
        let table = table(&definition, &metadata);
        ensure!(table.schema == owner, "备份对象不属于目标用户");
        qualified(&table)?;
        if definition.kind == "Table" {
            tables.insert(table.name);
        }
        let fields = definition.fields.as_deref().unwrap_or_default();
        ensure!(
            definition.kind != "Table" || !fields.is_empty(),
            "备份表缺少数据字段"
        );
        ensure!(fields.len() <= 4096, "备份字段超过4096项");
        for data in definition.data.as_deref().unwrap_or_default() {
            archive.verify_with(data, || control.check())?;
            let mut rows = archive.records_with_strings(&data.filename, standard(&definition))?;
            while let Some(row) = rows.next_record()? {
                control.check()?;
                super::super::oracle_values::parse(row, standard(&definition), fields.len())?;
            }
        }
    }
    // Dropping a selected table must not remove a foreign key on an unselected table.
    for name in &tables {
        let rows=workbench::metadata(conn,"SELECT c.owner,c.table_name FROM all_constraints c JOIN all_constraints r ON r.owner=c.r_owner AND r.constraint_name=c.r_constraint_name WHERE r.owner=:1 AND r.table_name=:2 AND c.constraint_type='R'",&[Cell::Text(owner.into()),Cell::Text(name.clone())]).await?;
        for row in rows {
            ensure!(
                workbench::text(&row, 0)? == owner && tables.contains(&workbench::text(&row, 1)?),
                "表 {name} 被未选中的表引用，请先包含这些依赖表再还原"
            );
        }
    }
    let mut records = 0u64;
    let result: Result<()> = async {
        for object in metadata.objects.iter().rev() {
            control.check()?;
            let definition: Definition = archive.json(&object.metadata.filename, true)?;
            let target = qualified(&table(&definition, &metadata))?;
            let kind = object_type(&definition)?;
            if let Err(error) = conn
                .execute(
                    format!(
                        "DROP {kind} {target}{}",
                        if kind == "TABLE" {
                            " CASCADE CONSTRAINTS"
                        } else {
                            ""
                        }
                    )
                    .as_str(),
                )
                .await
            {
                let message = error.to_string();
                if !message.contains("ORA-00942") && !message.contains("ORA-04043") {
                    return Err(error.into());
                }
            }
        }
        for kind in ["Table", "Function", "View"] {
            for object in metadata.objects.iter().filter(|object| object.kind == kind) {
                control.check()?;
                let definition: Definition = archive.json(&object.metadata.filename, true)?;
                control.log(&format!("还原 {kind}: {}", definition.name));
                execute(conn, &definition.ddl).await?;
                if kind == "Table" {
                    restore_table(
                        conn,
                        &mut archive,
                        &definition,
                        &metadata,
                        control,
                        committing,
                        &mut records,
                    )
                    .await?;
                }
            }
        }
        for phase in 0..3 {
            for object in &metadata.objects {
                control.check()?;
                let definition: Definition = archive.json(&object.metadata.filename, true)?;
                let statements = match phase {
                    0 => &definition.index_ddl,
                    1 => &definition.sub_ddl,
                    _ => &definition.trigger_ddl,
                };
                for sql in statements.as_deref().unwrap_or_default() {
                    if phase == 2
                        && let Some((create, alter)) = legacy_trigger_parts(sql)
                    {
                        execute(conn, create).await?;
                        execute(conn, alter).await?;
                    } else {
                        execute(conn, sql).await?;
                    }
                }
                if phase == 2 {
                    control.object();
                }
            }
        }
        for object in metadata
            .objects
            .iter()
            .filter(|object| matches!(object.kind.as_str(), "View" | "Function"))
        {
            let definition: Definition = archive.json(&object.metadata.filename, true)?;
            let target = qualified(&table(&definition, &metadata))?;
            conn.execute(format!("ALTER {} {target} COMPILE", object_type(&definition)?).as_str())
                .await?;
        }
        Ok(())
    }
    .await;
    result
        .context("Oracle DDL 可能已提交，失败或取消后请核对已还原对象；未确认的记录不会自动重试")?;
    Ok((path, metadata.objects.len() as u64, records))
}

#[cfg(test)]
mod tests {
    use super::legacy_trigger_parts;

    #[test]
    fn splits_existing_trigger_archive_at_its_own_state_statement() {
        let create = r#"CREATE OR REPLACE NONEDITIONABLE TRIGGER "A"."trigger name" BEFORE INSERT ON "A"."items" BEGIN NULL; END;"#;
        let disable = r#"ALTER TRIGGER "A"."trigger name" DISABLE"#;
        let old = format!("{create}\n{disable}\n");
        assert_eq!(legacy_trigger_parts(&old), Some((create, disable)));
        assert!(legacy_trigger_parts(&format!("{create}\n/\n{disable};")).is_none());
        assert!(
            legacy_trigger_parts(&format!("{create}\nALTER TRIGGER \"A\".\"other\" DISABLE"))
                .is_none()
        );
    }
}
