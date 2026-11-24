//! Fetch only named primary constraints in bounded batches; never join grant views.
use super::*;
struct Key {
    table: String,
    name: String,
}
pub(super) async fn read<T: Projection>(
    session: &mut AnyConnection,
    scope: &Scope<'_>,
    schema: &mut Schema<T>,
    lookup: &Index,
    mut scratch: usize,
    budget: usize,
    limit: usize,
) -> Result<()> {
    let (sql, bindings) = scope.statement(
        "table_name,constraint_name",
        "constraints",
        "constraint_type='P'",
    );
    let mut keys = Vec::new();
    {
        let mut rows = query(&sql, &bindings).fetch(&mut *session);
        let mut fetched = 0;
        while let Some(row) = rows.try_next().await? {
            fetched += 1;
            ensure!(
                fetched <= 10_000,
                "Oracle 主键超过 10,000 项，请缩小数据库范围"
            );
            if !scope.accepts(&row)? {
                continue;
            }
            let key = Key {
                table: scope.text(&row, 0)?,
                name: scope.text(&row, 1)?,
            };
            scratch += key.table.capacity() + key.name.capacity();
            let before = keys.capacity();
            keys.push(key);
            scratch += (keys.capacity() - before) * std::mem::size_of::<Key>();
            check(schema.retained, scratch, budget)?;
        }
    }
    let mut count = 0;
    for batch in keys.chunks(256) {
        let (mut sql, mut bindings) =
            scope.statement("table_name,column_name,constraint_name", "cons_columns", "");
        sql.push_str(if scope.own { " WHERE " } else { " AND " });
        sql.push_str("constraint_name IN (");
        for (index, key) in batch.iter().enumerate() {
            if index > 0 {
                sql.push(',');
            }
            bindings.push(Cell::Text(key.name.clone()));
            sql.push_str(&scope.parameter(bindings.len()));
        }
        sql.push(')');
        let mut rows = query(&sql, &bindings).fetch(&mut *session);
        while let Some(row) = rows.try_next().await? {
            count += 1;
            ensure!(
                count <= limit,
                "Oracle 主键字段超过 {limit} 项，请缩小数据库范围"
            );
            if !scope.accepts(&row)? {
                continue;
            }
            let table = scope.text(&row, 0)?;
            let name = scope.text(&row, 1)?;
            let constraint = scope.text(&row, 2)?;
            // Linguistic IN comparisons can also return a case-equivalent key
            // outside this batch. Attach only exact requested constraints.
            let Some(key) = batch.iter().find(|key| key.name == constraint) else {
                continue;
            };
            ensure!(key.table == table, "Oracle 主键所属表已变化，请刷新");
            if let Some((table, column)) = find(schema, lookup, &table, &name) {
                schema.tables[table].columns[column].mark_primary();
            }
        }
    }
    Ok(())
}
