//! Bounded schema projections without joins between expanded Oracle grant views.
use super::{metadata, text};
use crate::database::workbench::query;
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt;
use sqlx::AnyConnection;
use tomato_core::{database_objects::Field, workbench::Cell};
mod primary;
#[cfg(test)]
mod tests;

pub(in crate::database) use tomato_core::database_objects::Fields as Table;
pub(in crate::database) struct Schema<T> {
    pub tables: Vec<Table<T>>,
    pub retained: usize,
}
pub(in crate::database) struct Column {
    pub field: Field,
    pub data_type: String,
    pub primary: bool,
}
impl Column {
    pub fn heap_bytes(&self) -> usize {
        self.field.name.capacity() + self.field.comment.capacity() + self.data_type.capacity()
    }
}
// Object search retains the original compact Field allocation; completion also
// needs types and keys. Both use the same order, privilege and attribute lookup.
trait Projection {
    const TYPES_AND_KEYS: bool;
    fn new(name: String, data_type: String) -> Self;
    fn field(&self) -> &Field;
    fn field_mut(&mut self) -> &mut Field;
    fn heap_bytes(&self) -> usize;
    fn mark_primary(&mut self) {}
}
impl Projection for Field {
    const TYPES_AND_KEYS: bool = false;
    fn new(name: String, _: String) -> Self {
        Self {
            name,
            comment: String::new(),
        }
    }
    fn field(&self) -> &Field {
        self
    }
    fn field_mut(&mut self) -> &mut Field {
        self
    }
    fn heap_bytes(&self) -> usize {
        self.name.capacity() + self.comment.capacity()
    }
}
impl Projection for Column {
    const TYPES_AND_KEYS: bool = true;
    fn new(name: String, data_type: String) -> Self {
        Self {
            field: Field {
                name,
                comment: String::new(),
            },
            data_type,
            primary: false,
        }
    }
    fn field(&self) -> &Field {
        &self.field
    }
    fn field_mut(&mut self) -> &mut Field {
        &mut self.field
    }
    fn heap_bytes(&self) -> usize {
        self.heap_bytes()
    }
    fn mark_primary(&mut self) {
        self.primary = true;
    }
}
pub(super) struct Scope<'a> {
    pub(super) owner: &'a str,
    pub(super) own: bool,
    #[cfg(test)]
    pub(super) sqlite_parameters: bool,
}
impl<'a> Scope<'a> {
    pub(super) async fn new(session: &mut AnyConnection, owner: &'a str) -> Result<Self> {
        let identity = metadata(
            session,
            "SELECT SYS_CONTEXT('USERENV','SESSION_USER') FROM dual",
            &[],
        )
        .await?;
        let login = text(identity.first().context("无法读取 Oracle 会话身份")?, 0)?;
        Ok(Self {
            owner,
            own: owner == login,
            #[cfg(test)]
            sqlite_parameters: false,
        })
    }
    pub(super) fn accepts(&self, row: &sqlx::any::AnyRow) -> Result<bool> {
        Ok(self.own || text(row, 0)? == self.owner)
    }
    pub(super) fn text(&self, row: &sqlx::any::AnyRow, index: usize) -> Result<String> {
        text(row, index + usize::from(!self.own))
    }
    fn parameter(&self, index: usize) -> String {
        // SQLx SQLite rejects Oracle's :1 syntax. Only the synthetic dictionary
        // fixture uses ?1; native Oracle always exercises :1 in the server test.
        #[cfg(test)]
        if self.sqlite_parameters {
            return format!("?{index}");
        }
        format!(":{index}")
    }
    pub(super) fn statement(
        &self,
        columns: &str,
        view: &str,
        condition: &str,
    ) -> (String, Vec<Cell>) {
        let mut sql = format!(
            "SELECT {}{columns} FROM {}{view}",
            if self.own { "" } else { "owner," },
            if self.own { "user_" } else { "all_" }
        );
        let mut bindings = Vec::new();
        if !self.own {
            sql.push_str(" WHERE owner=");
            sql.push_str(&self.parameter(1));
            bindings.push(Cell::Text(self.owner.into()));
        }
        if !condition.is_empty() {
            sql.push_str(if self.own { " WHERE " } else { " AND " });
            sql.push_str(condition);
        }
        (sql, bindings)
    }
}
pub(in crate::database) async fn fields(
    session: &mut AnyConnection,
    owner: &str,
    budget: usize,
) -> Result<Schema<Field>> {
    read(session, owner, budget, 100_000).await
}
pub(in crate::database) async fn completion(
    session: &mut AnyConnection,
    owner: &str,
    budget: usize,
) -> Result<Schema<Column>> {
    read(session, owner, budget, 10_000).await
}
fn check(held: usize, extra: usize, budget: usize) -> Result<()> {
    ensure!(
        held.saturating_add(extra) <= budget,
        "Oracle 字段元数据超过 8 MiB，请缩小数据库范围"
    );
    Ok(())
}
type Index = Vec<(usize, usize)>;
fn index<T: Projection>(schema: &Schema<T>) -> Index {
    let mut result: Vec<_> = schema
        .tables
        .iter()
        .enumerate()
        .flat_map(|(table, value)| (0..value.columns.len()).map(move |column| (table, column)))
        .collect();
    result.sort_unstable_by(|&(at, ac), &(bt, bc)| {
        schema.tables[at]
            .name
            .cmp(&schema.tables[bt].name)
            .then_with(|| {
                schema.tables[at].columns[ac]
                    .field()
                    .name
                    .cmp(&schema.tables[bt].columns[bc].field().name)
            })
    });
    result
}
fn find<T: Projection>(
    schema: &Schema<T>,
    index: &Index,
    table: &str,
    column: &str,
) -> Option<(usize, usize)> {
    index
        .binary_search_by(|&(t, c)| {
            schema.tables[t].name.as_str().cmp(table).then_with(|| {
                schema.tables[t].columns[c]
                    .field()
                    .name
                    .as_str()
                    .cmp(column)
            })
        })
        .ok()
        .map(|at| index[at])
}
async fn read<T: Projection>(
    session: &mut AnyConnection,
    owner: &str,
    budget: usize,
    limit: usize,
) -> Result<Schema<T>> {
    let scope = Scope::new(session, owner).await?;
    project(session, &scope, budget, limit).await
}
async fn project<T: Projection>(
    session: &mut AnyConnection,
    scope: &Scope<'_>,
    budget: usize,
    limit: usize,
) -> Result<Schema<T>> {
    let mut result = Schema {
        tables: Vec::new(),
        retained: 0,
    };
    // A linguistic Oracle sort can interleave case-distinct table names at
    // each column_id. Index exact names instead of assuming contiguous groups.
    // Store positions only; table names and field vectors are never cloned.
    let mut table_names = Vec::<usize>::new();
    let columns = if T::TYPES_AND_KEYS {
        "table_name,column_name,data_type"
    } else {
        "table_name,column_name"
    };
    let (mut sql, bindings) = scope.statement(columns, "tab_columns", "");
    sql.push_str(" ORDER BY table_name,column_id");
    {
        let mut rows = query(&sql, &bindings).fetch(&mut *session);
        let mut count = 0;
        while let Some(row) = rows.try_next().await? {
            count += 1;
            ensure!(
                count <= limit,
                "Oracle 字段超过 {limit} 项，请缩小数据库范围"
            );
            if !scope.accepts(&row)? {
                continue;
            }
            let name = scope.text(&row, 0)?;
            let table =
                match table_names.binary_search_by(|&index| result.tables[index].name.cmp(&name)) {
                    Ok(position) => table_names[position],
                    Err(position) => {
                        let index = result.tables.len();
                        let before = result.tables.capacity();
                        result.retained += name.capacity();
                        result.tables.push(Table {
                            name,
                            columns: Vec::new(),
                        });
                        result.retained +=
                            (result.tables.capacity() - before) * std::mem::size_of::<Table<T>>();
                        table_names.insert(position, index);
                        index
                    }
                };
            let column = T::new(
                scope.text(&row, 1)?,
                if T::TYPES_AND_KEYS {
                    scope.text(&row, 2)?
                } else {
                    String::new()
                },
            );
            result.retained += column.heap_bytes();
            let columns = &mut result.tables[table].columns;
            let before = columns.capacity();
            columns.push(column);
            result.retained += (columns.capacity() - before) * std::mem::size_of::<T>();
            check(
                result.retained,
                table_names.capacity() * std::mem::size_of::<usize>(),
                budget,
            )?;
        }
    }
    drop(table_names);
    let lookup = index(&result);
    let scratch = lookup.capacity() * std::mem::size_of::<(usize, usize)>();
    check(result.retained, scratch, budget)?;
    let (sql, bindings) = scope.statement(
        "table_name,column_name,comments",
        "col_comments",
        "comments IS NOT NULL",
    );
    {
        let mut rows = query(&sql, &bindings).fetch(&mut *session);
        let mut count = 0;
        while let Some(row) = rows.try_next().await? {
            count += 1;
            ensure!(
                count <= 100_000,
                "Oracle 字段注释超过 100,000 项，请缩小数据库范围"
            );
            if !scope.accepts(&row)? {
                continue;
            }
            let table = scope.text(&row, 0)?;
            let name = scope.text(&row, 1)?;
            if let Some((table, column)) = find(&result, &lookup, &table, &name) {
                let field = result.tables[table].columns[column].field_mut();
                let comment = scope.text(&row, 2)?;
                result.retained = result
                    .retained
                    .saturating_sub(field.comment.capacity())
                    .saturating_add(comment.capacity());
                field.comment = comment;
                check(result.retained, scratch, budget)?;
            }
        }
    }
    if T::TYPES_AND_KEYS {
        primary::read(session, scope, &mut result, &lookup, scratch, budget, limit).await?;
    }
    Ok(result)
}
