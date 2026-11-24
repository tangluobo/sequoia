use super::*;
use std::collections::BTreeMap;
use tomato_core::ai_sql::editor::SchemaTable;
use tomato_core::sql_completion::{Metadata, Table};

pub(super) async fn read(
    conn: &mut AnyConnection,
    kind: &str,
    schema: &str,
    database: &str,
) -> Result<Metadata> {
    if kind == "HIVE" {
        return super::super::hive::catalog::completion(conn, schema).await;
    }
    let database = if kind == "POSTGRESQL" && database.is_empty() {
        let row = sqlx::query("SELECT current_database()::text")
            .fetch_one(&mut *conn)
            .await?;
        text(&row, 0)?
    } else {
        database.to_owned()
    };
    let databases = super::catalog::databases(conn, kind).await?;
    let mut result = Metadata {
        databases,
        schema: schema.into(),
        database: if kind == "POSTGRESQL" {
            database.clone()
        } else {
            schema.into()
        },
        namespaces: vec![schema.into()],
        ..Default::default()
    };
    let mut indices = BTreeMap::<String, usize>::new();
    let mut held = std::mem::size_of::<Metadata>()
        + result.schema.capacity()
        + result.database.capacity()
        + (result.namespaces.capacity() + result.databases.capacity())
            * std::mem::size_of::<String>()
        + result
            .namespaces
            .iter()
            .chain(&result.databases)
            .map(String::capacity)
            .sum::<usize>();
    if held > 8 * 1024 * 1024 {
        bail!("补全元数据超过 8 MiB，请缩小数据库范围");
    }
    if kind == "ORACLE" {
        let data = super::super::oracle::catalog::schema::completion(
            conn,
            schema,
            limits::QUERY_BYTES.saturating_sub(held),
        )
        .await?;
        let mut pending = data.retained;
        for table in data.tables {
            pending = pending.saturating_sub(table.name.capacity());
            let index = table_index(&mut result, &mut indices, &mut held, table.name);
            let buffer = table.columns.capacity()
                * std::mem::size_of::<super::super::oracle::catalog::schema::Column>();
            for column in table.columns {
                pending = pending.saturating_sub(column.heap_bytes());
                add_column(
                    &mut result,
                    &mut held,
                    index,
                    column.field.name,
                    &column.data_type,
                    column.primary,
                    column.field.comment,
                );
                drop(column.data_type);
                check(held, pending)?;
            }
            pending = pending.saturating_sub(buffer);
            check(held, pending)?;
        }
        return Ok(result);
    }
    let sql=match kind {
        "MYSQL"=>"SELECT t.TABLE_NAME,c.COLUMN_NAME,UPPER(c.DATA_TYPE),CASE WHEN c.COLUMN_KEY='PRI' THEN '1' ELSE '0' END,c.COLUMN_COMMENT FROM information_schema.TABLES t LEFT JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA=t.TABLE_SCHEMA AND c.TABLE_NAME=t.TABLE_NAME WHERE t.TABLE_SCHEMA=? ORDER BY t.TABLE_NAME,c.ORDINAL_POSITION".into(),
        "POSTGRESQL"=>"SELECT t.table_name::text,c.column_name::text,c.udt_name::text,CASE WHEN EXISTS(SELECT 1 FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class r ON r.oid=i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid=r.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid=r.oid AND a.attnum=ANY(i.indkey) WHERE i.indisprimary AND n.nspname=t.table_schema AND r.relname=t.table_name AND a.attname=c.column_name) THEN '1' ELSE '0' END,pg_catalog.col_description(pc.oid,c.ordinal_position)::text FROM information_schema.tables t LEFT JOIN information_schema.columns c ON c.table_schema=t.table_schema AND c.table_name=t.table_name LEFT JOIN pg_catalog.pg_namespace pn ON pn.nspname=t.table_schema LEFT JOIN pg_catalog.pg_class pc ON pc.relnamespace=pn.oid AND pc.relname=t.table_name WHERE t.table_schema=$1 ORDER BY t.table_name,c.ordinal_position".into(),
        _=>format!("SELECT m.name,p.name,p.type,CASE WHEN p.pk>0 THEN '1' ELSE '0' END,'' FROM {}.sqlite_master m LEFT JOIN pragma_table_xinfo(m.name,?) p ON true WHERE m.type IN ('table','view') AND m.name NOT LIKE 'sqlite_%' ORDER BY m.name,p.cid",crate::dialect::resolve(kind)?.quote(schema)?),
    };
    let mut stream = query(&sql, &[Cell::Text(schema.into())]).fetch(conn);
    let mut rows = 0;
    while let Some(row) = stream.try_next().await? {
        rows += 1;
        if rows > 10_000 {
            bail!("补全字段超过 10,000 项，请缩小数据库范围");
        }
        let name = text(&row, 0)?;
        let index = table_index(&mut result, &mut indices, &mut held, name);
        if let Some(name) = optional(&row, 1)? {
            let data_type = optional(&row, 2)?.unwrap_or_default();
            let primary = optional(&row, 3)?.is_some_and(|value| value == "1");
            let comment = optional(&row, 4)?.unwrap_or_default();
            add_column(
                &mut result,
                &mut held,
                index,
                name,
                &data_type,
                primary,
                comment,
            );
        }
        check(held, 0)?;
    }
    Ok(result)
}
fn check(held: usize, pending: usize) -> Result<()> {
    if held.saturating_add(pending) > limits::QUERY_BYTES {
        bail!("补全元数据超过 8 MiB，请缩小数据库范围");
    }
    Ok(())
}
fn table_index(
    result: &mut Metadata,
    indices: &mut BTreeMap<String, usize>,
    held: &mut usize,
    name: String,
) -> usize {
    if let Some(index) = indices.get(&name) {
        return *index;
    }
    let index = result.tables.len();
    *held += name.capacity() * 2 + std::mem::size_of::<(String, usize)>();
    indices.insert(name.clone(), index);
    let before = result.tables.capacity();
    result.tables.push(Table {
        name,
        columns: vec![],
    });
    *held += (result.tables.capacity() - before) * std::mem::size_of::<Table>();
    let name = &result.tables[index].name;
    let table = SchemaTable {
        text: format!("TABLE {name}"),
        terms: vec![name.clone()],
        order: index,
    };
    *held += ai_bytes(&table);
    let before = result.ai_tables.capacity();
    result.ai_tables.push(table);
    *held += (result.ai_tables.capacity() - before) * std::mem::size_of::<SchemaTable>();
    index
}
fn add_column(
    result: &mut Metadata,
    held: &mut usize,
    index: usize,
    name: String,
    data_type: &str,
    primary: bool,
    comment: String,
) {
    let table = &mut result.ai_tables[index];
    let before = ai_bytes(table);
    table.text.push_str("\n  ");
    table.text.push_str(&name);
    if !data_type.trim().is_empty() {
        table.text.push(' ');
        table.text.push_str(data_type);
    }
    if primary {
        table.text.push_str(" PRIMARY KEY");
    }
    if !comment.trim().is_empty() {
        table.text.push_str(" -- ");
        table
            .text
            .extend(comment.chars().map(|ch| if ch == '\n' { ' ' } else { ch }));
        table.terms.push(comment);
    }
    table.terms.push(name.clone());
    *held += ai_bytes(table) - before;
    let columns = &mut result.tables[index].columns;
    *held += name.capacity();
    let before = columns.capacity();
    columns.push(name);
    *held += (columns.capacity() - before) * std::mem::size_of::<String>();
}
fn ai_bytes(table: &SchemaTable) -> usize {
    table.text.capacity()
        + table.terms.capacity() * std::mem::size_of::<String>()
        + table.terms.iter().map(String::capacity).sum::<usize>()
}
