//! Restore keeps one definition and one row in memory and commits only after every phase succeeds.
use super::*;
use crate::backup::{Archive, Repository};
use sqlx::{Connection as _, Executor};
pub(in crate::database::backup) async fn restore(
    conn: &mut AnyConnection,
    root: &Path,
    request: &backup::Restore,
    control: &Control,
    committing: &std::sync::atomic::AtomicBool,
) -> Result<(std::path::PathBuf, u64, u64)> {
    ensure!(
        request.namespace.is_none(),
        "PostgreSQL nb3 按备份原 schema 还原，不支持静默重命名 schema"
    );
    let path = Repository::new(root.into()).read_path(&request.scope, &request.name)?;
    let mut archive = Archive::open(&path)?;
    let metadata: Metadata = archive.json("meta.json", false)?;
    ensure!(
        metadata.database_type == "POSTGRESQL" && metadata.encryption == "None",
        "备份类型或加密格式不匹配"
    );
    ensure!(
        !metadata.objects.is_empty() && metadata.objects.len() <= 10_000,
        "备份对象数无效"
    );
    control.start(metadata.objects.len() as u64);
    control.log("校验备份文件");
    let mut keys = Vec::new();
    let mut dependencies = Vec::new();
    let mut retained = 0usize;
    for object in &metadata.objects {
        control.check()?;
        archive.verify_with(&object.metadata, || control.check())?;
        let definition: Definition = archive.json(&object.metadata.filename, true)?;
        ensure!(
            definition.name == object.name
                && definition.kind == object.kind
                && matches!(definition.kind.as_str(), "Table" | "View" | "Function")
                && !definition.ddl.is_empty(),
            "备份对象定义缺失或不一致，保留数据库原对象"
        );
        let pg = definition
            .postgres
            .as_ref()
            .context("旧 PostgreSQL nb3 缺少可还原的原生对象定义，保留数据库原对象")?;
        let key = format!(
            "{}:{}",
            if definition.kind == "Function" {
                "routine"
            } else {
                "relation"
            },
            qualified(&TableId {
                schema: pg.schema.clone(),
                name: pg.name.clone(),
            })?
        );
        retained = retained
            .saturating_add(key.len() + pg.dependencies.iter().map(String::len).sum::<usize>());
        ensure!(
            retained <= 8 * 1024 * 1024 && !keys.contains(&key),
            "备份依赖超过8 MiB或对象重复"
        );
        keys.push(key);
        dependencies.push(pg.dependencies.clone());
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
                    "备份记录不是 SQL 值组"
                );
            }
        }
    }
    let indices = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.as_str(), i))
        .collect::<std::collections::HashMap<_, _>>();
    let mut outgoing = vec![Vec::new(); keys.len()];
    let mut incoming = vec![0usize; keys.len()];
    for (index, deps) in dependencies.iter().enumerate() {
        for &dependency in deps
            .iter()
            .filter_map(|dep| indices.get(dep.as_str()))
            .collect::<std::collections::HashSet<_>>()
        {
            outgoing[dependency].push(index);
            incoming[index] += 1;
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, &n)| (n == 0).then_some(i))
        .collect::<std::collections::VecDeque<_>>();
    let mut order = Vec::with_capacity(keys.len());
    while let Some(index) = ready.pop_front() {
        order.push(index);
        for &dependent in &outgoing[index] {
            incoming[dependent] -= 1;
            if incoming[dependent] == 0 {
                ready.push_back(dependent);
            }
        }
    }
    ensure!(order.len() == keys.len(), "备份对象存在循环结构依赖");
    let mut transaction = conn.begin().await?;
    transaction.execute("SET LOCAL search_path=pg_catalog; SET LOCAL check_function_bodies=off; SET LOCAL statement_timeout=0; SET LOCAL lock_timeout='3s'; SET LOCAL DateStyle='ISO'; SET LOCAL IntervalStyle='postgres'; SET LOCAL TimeZone='UTC'; SET LOCAL bytea_output='hex'; SET LOCAL extra_float_digits=3").await?;
    let result = apply(&mut transaction, &mut archive, &metadata, &order, control)
        .await
        .and_then(|records| {
            control.check()?;
            Ok(records)
        });
    match result {
        Ok(records) => {
            committing.store(true, std::sync::atomic::Ordering::Release);
            transaction.commit().await?;
            Ok((path, metadata.objects.len() as u64, records))
        }
        Err(error) => {
            transaction
                .rollback()
                .await
                .context("还原失败，事务回滚失败")?;
            Err(error)
        }
    }
}
async fn execute(conn: &mut AnyConnection, sql: &str, control: &Control) -> Result<()> {
    control.check()?;
    tokio::select! {result=conn.execute(sql)=>{result?;},()=control.wait_cancelled()=>{anyhow::bail!("操作已取消，正在回滚还原事务")}}
    Ok(())
}
async fn apply(
    conn: &mut AnyConnection,
    archive: &mut Archive,
    metadata: &Metadata,
    order: &[usize],
    control: &Control,
) -> Result<u64> {
    // Remove only constraints owned by selected tables; external dependencies block and roll back.
    for &index in order {
        let definition: Definition =
            archive.json(&metadata.objects[index].metadata.filename, true)?;
        if definition.kind != "Table" {
            continue;
        }
        let pg = definition.postgres.as_ref().unwrap();
        let table = TableId {
            schema: pg.schema.clone(),
            name: pg.name.clone(),
        };
        for row in rows(conn,"SELECT quote_ident(con.conname) FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND con.contype='f' AND con.conislocal",&table).await? {
            execute(conn,&format!("ALTER TABLE {} DROP CONSTRAINT {}",qualified(&table)?,workbench::text(&row,0)?),control).await?;
        }
    }
    for &index in order.iter().rev() {
        let definition: Definition =
            archive.json(&metadata.objects[index].metadata.filename, true)?;
        {
            execute(
                conn,
                &definition.postgres.as_ref().unwrap().drop_sql,
                control,
            )
            .await?;
        }
    }
    // SubDDL contains schemas, enum definitions and owned serial sequences.
    for &index in order {
        let definition: Definition =
            archive.json(&metadata.objects[index].metadata.filename, true)?;
        for ddl in definition.sub_ddl.as_deref().unwrap_or_default() {
            execute(conn, ddl, control).await?;
        }
    }
    let mut records = 0u64;
    {
        for &index in order {
            let definition: Definition =
                archive.json(&metadata.objects[index].metadata.filename, true)?;
            let kind = definition.kind.as_str();
            control.log(&format!("还原 {kind}: {}", definition.name));
            execute(conn, &definition.ddl, control).await?;
            if kind == "Table" {
                let pg = definition.postgres.as_ref().unwrap();
                let target = qualified(&TableId {
                    schema: pg.schema.clone(),
                    name: pg.name.clone(),
                })?;
                let fields = definition.fields.as_deref().context("备份缺少字段")?;
                let prefix = if fields.is_empty() {
                    format!("INSERT INTO {target} DEFAULT VALUES")
                } else {
                    format!(
                        "INSERT INTO {target} ({}) OVERRIDING SYSTEM VALUE VALUES ",
                        fields
                            .iter()
                            .map(|f| quote(f))
                            .collect::<Result<Vec<_>>>()?
                            .join(",")
                    )
                };
                let mut statement = String::new();
                let mut pending = 0;
                for data in definition.data.as_deref().unwrap_or_default() {
                    let mut rows = archive.records(&data.filename)?;
                    while let Some(row) = rows.next_record()? {
                        statement.clear();
                        statement.push_str(&prefix);
                        if !fields.is_empty() {
                            statement.push_str(row);
                        }
                        execute(conn, &statement, control).await?;
                        records += 1;
                        pending += 1;
                        if pending == 256 {
                            control.records(pending);
                            pending = 0;
                        }
                    }
                }
                control.records(pending);
            }
        }
    }
    for &index in order {
        let definition: Definition =
            archive.json(&metadata.objects[index].metadata.filename, true)?;
        for ddl in definition.index_ddl.as_deref().unwrap_or_default() {
            execute(conn, ddl, control).await?;
        }
    }
    for &index in order {
        let definition: Definition =
            archive.json(&metadata.objects[index].metadata.filename, true)?;
        for ddl in definition.trigger_ddl.as_deref().unwrap_or_default() {
            execute(conn, ddl, control).await?;
        }
        control.object();
    }
    Ok(records)
}
