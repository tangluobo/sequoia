//! One bounded field projection serves both complete and background catalogs.
use super::*;
use tomato_core::database_objects::{Field, Fields};

pub(in crate::database::objects) async fn read(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
    catalog: &Catalog,
    budget: usize,
    best_effort: bool,
) -> Result<Vec<Fields>> {
    if catalog.objects.is_empty() {
        return Ok(Vec::new());
    }
    let mut targets: Vec<_> = (0..catalog.objects.len()).collect();
    targets.sort_unstable_by(|&a, &b| {
        catalog.objects[a]
            .table
            .name
            .cmp(&catalog.objects[b].table.name)
    });
    let target_bytes = targets.capacity() * std::mem::size_of::<usize>();
    ensure!(
        target_bytes <= budget,
        "对象字段超过 8 MiB，请缩小数据库范围"
    );
    let find = |name: &str| {
        targets
            .binary_search_by(|&index| catalog.objects[index].table.name.as_str().cmp(name))
            .ok()
            .map(|at| targets[at])
    };
    if kind == "ORACLE" {
        let mut result = super::super::super::oracle::catalog::schema::fields(
            session,
            schema,
            budget - target_bytes,
        )
        .await?;
        result.tables.retain(|table| find(&table.name).is_some());
        return Ok(result.tables);
    }
    let mut result = Vec::<Fields>::new();
    let mut retained = 0usize;
    let mut count = 0usize;
    if kind == "HIVE" {
        for object in &catalog.objects {
            let definition =
                match super::super::super::hive::catalog::describe(session, &object.table).await {
                    Ok(value) => value,
                    Err(_) if best_effort => continue,
                    Err(error) => return Err(error),
                };
            let before = result.capacity();
            result.push(Fields {
                name: object.table.name.clone(),
                columns: Vec::new(),
            });
            retained += (result.capacity() - before) * std::mem::size_of::<Fields>();
            let table = result.last_mut().expect("just inserted");
            retained += table.name.capacity();
            ensure!(
                retained.saturating_add(target_bytes) <= budget,
                "对象字段超过 8 MiB，请缩小数据库范围"
            );
            let mut comments = definition.comments;
            for column in definition.columns {
                count += 1;
                ensure!(
                    count <= 100_000,
                    "对象字段超过 100,000 项，请缩小数据库范围"
                );
                let field = Field {
                    comment: comments.remove(&column.name).unwrap_or_default(),
                    name: column.name,
                };
                push(table, field, &mut retained);
                ensure!(
                    retained.saturating_add(target_bytes) <= budget,
                    "对象字段超过 8 MiB，请缩小数据库范围"
                );
            }
        }
        return Ok(result);
    }
    let sql = match kind {
        "MYSQL" => {
            "SELECT TABLE_NAME,COLUMN_NAME,COALESCE(COLUMN_COMMENT,'') FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME,ORDINAL_POSITION"
        }
        "POSTGRESQL" => {
            "SELECT c.relname::text,a.attname::text,COALESCE(pg_catalog.col_description(c.oid,a.attnum),'') FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relkind IN ('r','p','v','m') AND a.attnum>0 AND NOT a.attisdropped ORDER BY c.relname,a.attnum"
        }
        _ => bail!("当前数据库尚未实现对象字段搜索"),
    };
    let mut positions = vec![usize::MAX; catalog.objects.len()];
    let scratch = target_bytes + positions.capacity() * std::mem::size_of::<usize>();
    ensure!(scratch <= budget, "对象字段超过 8 MiB，请缩小数据库范围");
    let mut rows = sqlx::query(sql).bind(schema).fetch(session);
    while let Some(row) = rows.try_next().await? {
        count += 1;
        ensure!(
            count <= 100_000,
            "对象字段超过 100,000 项，请缩小数据库范围"
        );
        let name = workbench::text(&row, 0)?;
        let Some(target) = find(&name) else {
            continue;
        };
        let position = if positions[target] != usize::MAX {
            positions[target]
        } else {
            let index = result.len();
            let before = result.capacity();
            retained += name.capacity();
            result.push(Fields {
                name,
                columns: Vec::new(),
            });
            retained += (result.capacity() - before) * std::mem::size_of::<Fields>();
            positions[target] = index;
            index
        };
        push(
            &mut result[position],
            Field {
                name: workbench::text(&row, 1)?,
                comment: workbench::text(&row, 2)?,
            },
            &mut retained,
        );
        ensure!(
            retained.saturating_add(scratch) <= budget,
            "对象字段超过 8 MiB，请缩小数据库范围"
        );
    }
    Ok(result)
}
fn push(table: &mut Fields, field: Field, retained: &mut usize) {
    *retained += field.name.capacity() + field.comment.capacity();
    let before = table.columns.capacity();
    table.columns.push(field);
    *retained += (table.columns.capacity() - before) * std::mem::size_of::<Field>();
}
