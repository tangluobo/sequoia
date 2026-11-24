//! Bounded object-browser catalog loading on the existing database actor/session.
use super::workbench;
use anyhow::{Result, bail, ensure};
use futures_util::TryStreamExt;
use sqlx::AnyConnection;
use tomato_core::{
    database_objects::{Catalog, ForeignKey, Object, data_length},
    workbench::TableId,
};

const BUDGET: usize = 8 * 1024 * 1024;
const MAX_OBJECTS: usize = 10_000;
mod create;
pub(super) mod designer;
mod designer_foreign_keys;
mod designer_indexes;
mod fields;
mod metadata;
mod mutate;
mod oracle_designer;
pub(super) use create::execute as create_table;
pub(super) use metadata::read as design_metadata;
pub(super) use mutate::execute as change;

pub(super) async fn read(session: &mut AnyConnection, kind: &str, schema: &str) -> Result<Catalog> {
    let (mut catalog, mut retained) = read_headers(session, kind, schema).await?;
    fields::read(session, kind, schema, &mut catalog, &mut retained).await?;
    Ok(catalog)
}

pub(super) async fn headers(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
) -> Result<Catalog> {
    read_headers(session, kind, schema)
        .await
        .map(|(catalog, _)| catalog)
}

pub(super) async fn preload(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
    catalog: &Catalog,
) -> Result<Vec<tomato_core::database_objects::Fields>> {
    ensure!(catalog.objects.len() <= MAX_OBJECTS, "对象超过 10,000 项");
    ensure!(
        catalog.objects.iter().all(|o| o.table.schema == schema),
        "字段加载的数据库范围已变化，请刷新"
    );
    let retained = catalog.retained_bytes();
    ensure!(retained <= BUDGET, "对象目录超过 8 MiB，请缩小数据库范围");
    fields::load::read(session, kind, schema, catalog, BUDGET - retained, true).await
}

async fn read_headers(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
) -> Result<(Catalog, usize)> {
    if kind == "HIVE" {
        let catalog = super::hive::catalog::objects(session, schema).await?;
        let retained = catalog.retained_bytes();
        ensure!(retained <= BUDGET, "对象目录超过 8 MiB，请缩小数据库范围");
        return Ok((catalog, retained));
    }
    if kind == "ORACLE" {
        let (mut catalog, mut retained) =
            super::oracle::catalog::read_objects(session, schema, BUDGET).await?;
        for key in super::oracle::catalog::read_foreign_keys(session, schema, None).await? {
            retained += key.retained_bytes();
            let previous = catalog.foreign_keys.capacity();
            catalog.foreign_keys.push(key.value);
            retained +=
                (catalog.foreign_keys.capacity() - previous) * std::mem::size_of::<ForeignKey>();
            ensure!(retained <= BUDGET, "对象目录超过 8 MiB，请缩小数据库范围");
        }
        return Ok((catalog, retained));
    }
    let sql = match kind {
        "MYSQL" => {
            "SELECT TABLE_NAME,TABLE_TYPE,COALESCE(TABLE_COMMENT,''),COALESCE(ENGINE,''),COALESCE(CAST(AUTO_INCREMENT AS CHAR),''),COALESCE(DATE_FORMAT(UPDATE_TIME,'%Y-%m-%d %H:%i:%s'),''),CAST((COALESCE(DATA_LENGTH,0)+COALESCE(INDEX_LENGTH,0)) AS CHAR),COALESCE(CAST(TABLE_ROWS AS CHAR),'') FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_TYPE,TABLE_NAME"
        }
        // The Java query calls a nonstandard pg_stat_last_operation_time function,
        // but discards its value. Return the same empty display field on stock PG.
        "POSTGRESQL" => {
            "SELECT c.relname::text,CASE WHEN c.relkind IN ('v','m') THEN 'VIEW' ELSE 'TABLE' END,COALESCE(pg_catalog.obj_description(c.oid),''),'','','',CASE WHEN c.relkind IN ('v','m') THEN '0' ELSE pg_catalog.pg_total_relation_size(c.oid)::text END,CASE WHEN c.relkind IN ('v','m') THEN '' ELSE c.reltuples::bigint::text END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relkind IN ('r','p','v','m') ORDER BY CASE WHEN c.relkind IN ('v','m') THEN 1 ELSE 0 END,c.relname"
        }
        _ => bail!("当前数据库尚未实现对象详细目录"),
    };
    let mut retained = 0usize;
    let mut catalog = Catalog::default();
    {
        let mut stream = sqlx::query(sql).bind(schema).fetch(&mut *session);
        while let Some(row) = stream.try_next().await? {
            ensure!(
                catalog.objects.len() < MAX_OBJECTS,
                "对象超过 10,000 项，请缩小数据库范围"
            );
            let field = |index| workbench::text(&row, index);
            let object = Object {
                table: TableId {
                    schema: schema.into(),
                    name: field(0)?,
                },
                view: field(1)?.eq_ignore_ascii_case("VIEW"),
                comment: field(2)?,
                engine: field(3)?,
                auto_increment: field(4)?,
                update_time: field(5)?,
                data_length: data_length(field(6)?.parse()?),
                rows: field(7)?,
                fields: Vec::new(),
            };
            retained = retained
                .saturating_add(object.table.schema.capacity())
                .saturating_add(object.table.name.capacity())
                .saturating_add(object.comment.capacity())
                .saturating_add(object.engine.capacity())
                .saturating_add(object.auto_increment.capacity())
                .saturating_add(object.update_time.capacity())
                .saturating_add(object.data_length.capacity())
                .saturating_add(object.rows.capacity());
            let previous = catalog.objects.capacity();
            catalog.objects.push(object);
            retained = retained.saturating_add(
                (catalog.objects.capacity() - previous) * std::mem::size_of::<Object>(),
            );
            ensure!(retained <= BUDGET, "对象目录超过 8 MiB，请缩小数据库范围");
        }
    }
    let sql = match kind {
        "MYSQL" => {
            "SELECT TABLE_NAME,CONSTRAINT_NAME,COLUMN_NAME,REFERENCED_TABLE_SCHEMA,REFERENCED_TABLE_NAME,REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA=? AND REFERENCED_TABLE_NAME IS NOT NULL ORDER BY TABLE_NAME,CONSTRAINT_NAME,ORDINAL_POSITION"
        }
        "POSTGRESQL" => {
            "SELECT c.relname::text,k.conname::text,a.attname::text,rn.nspname::text,r.relname::text,ra.attname::text FROM pg_catalog.pg_constraint k JOIN pg_catalog.pg_class c ON c.oid=k.conrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace JOIN pg_catalog.pg_class r ON r.oid=k.confrelid JOIN pg_catalog.pg_namespace rn ON rn.oid=r.relnamespace JOIN LATERAL unnest(k.conkey,k.confkey) WITH ORDINALITY u(a,b,position) ON true JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum=u.a JOIN pg_catalog.pg_attribute ra ON ra.attrelid=r.oid AND ra.attnum=u.b WHERE n.nspname=$1 AND k.contype='f' ORDER BY c.relname,k.conname,u.position"
        }
        _ => unreachable!(),
    };
    let mut stream = sqlx::query(sql).bind(schema).fetch(&mut *session);
    let mut columns = 0usize;
    while let Some(row) = stream.try_next().await? {
        columns += 1;
        ensure!(columns <= MAX_OBJECTS, "外键字段超过 10,000 项");
        let values = (0..6)
            .map(|index| workbench::text(&row, index))
            .collect::<Result<Vec<_>>>()?;
        // Keep each component separately. Splitting Java's GROUP_CONCAT would
        // corrupt legal column identifiers containing commas and truncate long keys.
        let [table, name, column, ref_schema, ref_table, ref_column]: [String; 6] = values
            .try_into()
            .map_err(|_| anyhow::anyhow!("外键元数据列数错误"))?;
        retained = retained
            .saturating_add(column.capacity())
            .saturating_add(ref_column.capacity());
        if let Some(last) = catalog
            .foreign_keys
            .last_mut()
            .filter(|last| last.table.name == table && last.name == name)
        {
            ensure!(
                last.referenced.schema == ref_schema && last.referenced.name == ref_table,
                "外键目标不一致"
            );
            let before = last.columns.capacity() + last.referenced_columns.capacity();
            last.columns.push(column);
            last.referenced_columns.push(ref_column);
            retained = retained.saturating_add(
                (last.columns.capacity() + last.referenced_columns.capacity() - before)
                    * std::mem::size_of::<String>(),
            );
        } else {
            let previous = catalog.foreign_keys.capacity();
            retained = retained
                .saturating_add(table.capacity())
                .saturating_add(name.capacity())
                .saturating_add(ref_schema.capacity())
                .saturating_add(ref_table.capacity())
                .saturating_add(schema.len())
                .saturating_add(2 * std::mem::size_of::<String>());
            catalog.foreign_keys.push(ForeignKey {
                name,
                table: TableId {
                    schema: schema.into(),
                    name: table,
                },
                columns: vec![column],
                referenced: TableId {
                    schema: ref_schema,
                    name: ref_table,
                },
                referenced_columns: vec![ref_column],
            });
            retained = retained.saturating_add(
                (catalog.foreign_keys.capacity() - previous) * std::mem::size_of::<ForeignKey>(),
            );
        }
        ensure!(retained <= BUDGET, "对象目录超过 8 MiB，请缩小数据库范围");
    }
    drop(stream);
    Ok((catalog, retained))
}
