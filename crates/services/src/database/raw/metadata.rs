use super::*;
use sqlx::{mysql::MySqlColumn, postgres::PgColumn};
use std::collections::HashMap;

pub(super) fn mysql(columns: &[MySqlColumn], result: &mut QueryResult) -> usize {
    result.has_result_set = true;
    result.columns = columns
        .iter()
        .map(|column| column.name().to_owned())
        .collect();
    result.column_types = columns
        .iter()
        .map(|column| mysql_type(column.type_info().name()))
        .collect();
    result.origins = columns
        .iter()
        .map(|column| ColumnOrigin {
            column: column.source_name().unwrap_or(column.name()).into(),
            table: column.source_table().unwrap_or_default().into(),
            catalog: column.source_database().unwrap_or_default().into(),
            ..Default::default()
        })
        .collect();
    result.retained_bytes()
}
pub(super) fn postgres(columns: &[PgColumn], result: &mut QueryResult) -> usize {
    result.has_result_set = true;
    result.columns = columns
        .iter()
        .map(|column| column.name().to_owned())
        .collect();
    result.column_types = columns
        .iter()
        .map(|column| postgres_type(column.type_info().name()))
        .collect();
    result.origins = columns
        .iter()
        .map(|column| ColumnOrigin {
            pg_reference: column
                .relation_id()
                .zip(column.relation_attribute_no())
                .filter(|(id, number)| id.0 > 0 && *number != 0)
                .map(|(id, number)| (id.0, number)),
            ..Default::default()
        })
        .collect();
    result.retained_bytes()
}
fn mysql_type(name: &str) -> i32 {
    match name.strip_suffix(" UNSIGNED").unwrap_or(name) {
        "TINYINT" => -6,
        "SMALLINT" => 5,
        "MEDIUMINT" | "INT" => 4,
        "BIGINT" => -5,
        "FLOAT" => 7,
        "DOUBLE" => 8,
        "DECIMAL" => 3,
        "DATE" | "YEAR" => 91,
        "TIME" => 92,
        "DATETIME" | "TIMESTAMP" => 93,
        "BIT" | "BOOLEAN" => -7,
        "BINARY" => -2,
        "VARBINARY" => -3,
        "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => -4,
        "CHAR" | "ENUM" | "SET" => 1,
        "VARCHAR" | "TINYTEXT" => 12,
        "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "JSON" => -1,
        "NULL" => 0,
        _ => 1111,
    }
}
fn postgres_type(name: &str) -> i32 {
    if name.ends_with("[]") {
        return 2003;
    }
    match name {
        "BOOL" => -7,
        "INT2" => 5,
        "INT4" => 4,
        "INT8" => -5,
        "FLOAT4" => 7,
        "FLOAT8" => 8,
        "NUMERIC" => 2,
        "DATE" => 91,
        "TIME" | "TIMETZ" => 92,
        "TIMESTAMP" | "TIMESTAMPTZ" => 93,
        "CHAR" | "BPCHAR" => 1,
        "VARCHAR" | "TEXT" | "NAME" => 12,
        "BYTEA" => -2,
        "BIT" => -7,
        _ => 1111,
    }
}
pub(super) async fn resolve_postgres(
    conn: &mut PgConnection,
    results: &mut [QueryResult],
) -> Result<()> {
    let mut positions = HashMap::<(u32, i16), Vec<(usize, usize)>>::new();
    for (result_index, result) in results.iter().enumerate() {
        for (column_index, column) in result.origins.iter().enumerate() {
            if let Some(reference) = column.pg_reference {
                positions
                    .entry(reference)
                    .or_default()
                    .push((result_index, column_index));
            }
        }
    }
    if positions.is_empty() {
        return Ok(());
    }
    let mut references: Vec<_> = positions.keys().copied().collect();
    references.sort_unstable();
    let retained = results.iter().fold(0usize, |total, result| {
        total.saturating_add(result.retained_bytes())
    });
    let mut remaining = limits::QUERY_BYTES.saturating_sub(retained);
    for chunk in references.chunks(512) {
        use std::fmt::Write;
        let mut query = String::from(
            "SELECT a.attrelid::bigint AS relation_oid,a.attnum::int AS attribute_no,a.attname::text AS column_name,c.relname::text AS table_name,n.nspname::text AS schema_name FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE NOT a.attisdropped AND (a.attrelid,a.attnum) IN (",
        );
        for (index, (relation, attribute)) in chunk.iter().enumerate() {
            if index > 0 {
                query.push(',');
            }
            write!(query, "({relation}::oid,{attribute}::int2)")?;
        }
        query.push(')');
        let mut rows = sqlx::query(&query).persistent(false).fetch(&mut *conn);
        while let Some(row) = rows.try_next().await? {
            let reference = (
                u32::try_from(row.try_get::<i64, _>("relation_oid")?)?,
                i16::try_from(row.try_get::<i32, _>("attribute_no")?)?,
            );
            let column: String = row.try_get("column_name")?;
            let table: String = row.try_get("table_name")?;
            let schema: String = row.try_get("schema_name")?;
            let bytes = column
                .len()
                .saturating_add(table.len())
                .saturating_add(schema.len());
            for &(result_index, column_index) in positions.get(&reference).into_iter().flatten() {
                let result = &mut results[result_index];
                if bytes > remaining {
                    result.origin_error = Some("来源信息超过结果保留预算，未映射列保持只读".into());
                    continue;
                }
                remaining -= bytes;
                let origin = &mut result.origins[column_index];
                origin.column = column.clone();
                origin.table = table.clone();
                origin.schema = schema.clone();
            }
        }
    }
    Ok(())
}
