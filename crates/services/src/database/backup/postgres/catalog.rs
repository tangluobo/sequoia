use super::*;
pub(in crate::database::backup) async fn list(
    conn: &mut AnyConnection,
    schema: Option<&str>,
) -> Result<Vec<Selection>> {
    use sqlx::Executor as _;
    let previous = sqlx::query("SELECT pg_catalog.current_setting('search_path')")
        .fetch_one(&mut *conn)
        .await?;
    let previous = workbench::text(&previous, 0)?;
    conn.execute("SET search_path=pg_catalog").await?;
    let result = list_inner(conn, schema).await;
    let cleanup = sqlx::query("SELECT pg_catalog.set_config('search_path',$1,false)")
        .bind(previous)
        .execute(&mut *conn)
        .await;
    let result = result?;
    cleanup?;
    Ok(result)
}
async fn list_inner(conn: &mut AnyConnection, schema: Option<&str>) -> Result<Vec<Selection>> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for (kind, sql) in [
        (
            Kind::Table,
            "SELECT n.nspname::text,c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp_%' ORDER BY n.nspname,c.relname",
        ),
        (
            Kind::View,
            "SELECT n.nspname::text,c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind='v' AND n.nspname NOT IN ('pg_catalog','information_schema') ORDER BY n.nspname,c.relname",
        ),
        (
            Kind::Function,
            "SELECT n.nspname::text,p.proname||'('||pg_get_function_identity_arguments(p.oid)||')' FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE p.prokind IN ('f','p') AND n.nspname NOT IN ('pg_catalog','information_schema') AND NOT EXISTS(SELECT 1 FROM pg_depend d WHERE d.classid='pg_proc'::regclass AND d.objid=p.oid AND d.deptype='e') ORDER BY n.nspname,p.proname,p.oid",
        ),
    ] {
        let (query, order) = sql.split_once(" ORDER BY ").context("对象目录缺少排序")?;
        let query = format!("{query} AND ($1::text IS NULL OR n.nspname=$1) ORDER BY {order}");
        let mut stream = sqlx::query(&query).bind(schema).fetch(&mut *conn);
        while let Some(row) = stream.try_next().await? {
            let schema = workbench::text(&row, 0)?;
            let name = workbench::text(&row, 1)?;
            bytes =
                bytes.saturating_add(schema.len() + name.len() + std::mem::size_of::<Selection>());
            ensure!(
                result.len() < 10_000 && bytes <= 8 * 1024 * 1024,
                "备份对象超过 10,000 项或 8 MiB"
            );
            result.push(Selection {
                kind,
                table: TableId { schema, name },
            });
        }
    }
    Ok(result)
}
pub(super) async fn definition(
    conn: &mut AnyConnection,
    selected: &Selection,
) -> Result<Definition> {
    ensure!(
        !selected.table.schema.is_empty()
            && !selected.table.schema.starts_with("pg_")
            && selected.table.schema != "information_schema",
        "不能备份系统 schema"
    );
    let mut result = Definition {
        meta_version: "30100".into(),
        name: display(selected),
        kind: selected.kind.archive_name().into(),
        ddl: String::new(),
        sub_ddl: Some(vec![format!(
            "CREATE SCHEMA IF NOT EXISTS {}",
            quote(&selected.table.schema)?
        )]),
        auto_increment: None,
        fields: None,
        trigger_ddl: Some(Vec::new()),
        index_ddl: Some(Vec::new()),
        data: None,
        oracle: None,
        postgres: Some(PostgresDefinition {
            schema: selected.table.schema.clone(),
            name: selected.table.name.clone(),
            drop_sql: String::new(),
            dependencies: Vec::new(),
        }),
    };
    match selected.kind {
        Kind::Table => table::definition(conn, &selected.table, &mut result).await?,
        Kind::View => {
            let sql = "SELECT pg_get_viewdef(c.oid,false),COALESCE(quote_literal(obj_description(c.oid)),''),COALESCE(array_to_string(c.reloptions,','),'') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind='v'";
            let values = rows(conn, sql, &selected.table).await?;
            let row = values.first().context("视图不存在")?;
            let name = qualified(&selected.table)?;
            let options = workbench::text(row, 2)?;
            result.ddl = format!(
                "CREATE VIEW {name} {} AS {}",
                if options.is_empty() {
                    String::new()
                } else {
                    format!("WITH ({options})")
                },
                workbench::text(row, 0)?
            );
            result.postgres.as_mut().unwrap().drop_sql = format!("DROP VIEW IF EXISTS {name}");
            let comment = workbench::text(row, 1)?;
            if !comment.is_empty() {
                result
                    .index_ddl
                    .as_mut()
                    .unwrap()
                    .push(format!("COMMENT ON VIEW {name} IS {comment}"));
            }
            let deps=rows(conn,"SELECT DISTINCT ns.nspname::text,ref.relname::text FROM pg_rewrite r JOIN pg_class v ON v.oid=r.ev_class JOIN pg_namespace n ON n.oid=v.relnamespace JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=r.oid JOIN pg_class ref ON ref.oid=d.refobjid AND d.refclassid='pg_class'::regclass JOIN pg_namespace ns ON ns.oid=ref.relnamespace WHERE n.nspname=$1 AND v.relname=$2 AND ref.oid<>v.oid AND ref.relkind IN ('r','p','v')",&selected.table).await?;
            for row in deps {
                result.postgres.as_mut().unwrap().dependencies.push(format!(
                    "relation:{}",
                    qualified(&TableId {
                        schema: workbench::text(&row, 0)?,
                        name: workbench::text(&row, 1)?,
                    })?
                ));
            }
            result
                .postgres
                .as_mut()
                .unwrap()
                .dependencies
                .extend(dependencies::view_functions(conn, &selected.table).await?);
        }
        Kind::Function => {
            let values=rows(conn,"SELECT pg_get_functiondef(p.oid),quote_ident(p.proname)||'('||pg_get_function_identity_arguments(p.oid)||')',p.prokind::text,COALESCE(quote_literal(obj_description(p.oid,'pg_proc')),'') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname=$1 AND p.proname||'('||pg_get_function_identity_arguments(p.oid)||')'=$2 AND p.prokind IN ('f','p')",&selected.table).await?;
            let row = values.first().context("函数不存在")?;
            result.ddl = workbench::text(row, 0)?;
            let mut seen = std::collections::HashSet::new();
            for row in rows(conn,"SELECT DISTINCT unnest(COALESCE(p.proallargtypes,p.proargtypes::oid[])||ARRAY[p.prorettype])::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname=$1 AND p.proname||'('||pg_get_function_identity_arguments(p.oid)||')'=$2",&selected.table).await? {
                types::append(conn,workbench::text(&row,0)?.parse()?,result.sub_ddl.as_mut().unwrap(),&mut seen,0).await?;
            }
            let kind = if workbench::text(row, 2)? == "p" {
                "PROCEDURE"
            } else {
                "FUNCTION"
            };
            let name = format!(
                "{}.{}",
                quote(&selected.table.schema)?,
                workbench::text(row, 1)?
            );
            result.postgres.as_mut().unwrap().drop_sql = format!("DROP {kind} IF EXISTS {name}");
            let comment = workbench::text(row, 3)?;
            if !comment.is_empty() {
                result
                    .index_ddl
                    .as_mut()
                    .unwrap()
                    .push(format!("COMMENT ON {kind} {name} IS {comment}"));
            }
            result
                .postgres
                .as_mut()
                .unwrap()
                .dependencies
                .extend(dependencies::function(conn, &selected.table).await?);
        }
        Kind::Event => anyhow::bail!("PostgreSQL 不支持 MySQL 事件对象"),
    }
    ensure!(
        !result.ddl.is_empty() && serde_json::to_vec(&result)?.len() <= 8 * 1024 * 1024,
        "对象定义为空或超过8 MiB"
    );
    Ok(result)
}
