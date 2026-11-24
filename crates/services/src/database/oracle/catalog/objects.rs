//! Read object headers separately from comments; dictionary joins can multiply
//! case-distinct identifiers under NLS_COMP=LINGUISTIC.
use super::schema::Scope;
use crate::database::workbench::query;
use anyhow::{Result, ensure};
use futures_util::TryStreamExt;
use sqlx::AnyConnection;
use tomato_core::{
    database_objects::{Catalog, Object, data_length},
    workbench::TableId,
};
#[cfg(test)]
mod tests;

pub(in crate::database) async fn read(
    session: &mut AnyConnection,
    owner: &str,
    budget: usize,
) -> Result<(Catalog, usize)> {
    let scope = Scope::new(session, owner).await?;
    project(session, &scope, budget).await
}
fn check(held: usize, scratch: usize, budget: usize) -> Result<()> {
    ensure!(
        held.saturating_add(scratch) <= budget,
        "对象目录超过 8 MiB，请缩小数据库范围"
    );
    Ok(())
}
async fn project(
    session: &mut AnyConnection,
    scope: &Scope<'_>,
    budget: usize,
) -> Result<(Catalog, usize)> {
    let mut catalog = Catalog::default();
    let mut retained = 0usize;
    let time = "TO_CHAR(last_analyzed,'YYYY-MM-DD HH24:MI:SS')";
    // The synthetic dictionary tests exercise projection and ownership, not
    // Oracle's date formatter. The actual Oracle fixture executes TO_CHAR.
    #[cfg(test)]
    let time = if scope.sqlite_parameters {
        "last_analyzed"
    } else {
        time
    };
    for (view, columns, source, order) in [
        (
            false,
            format!("table_name,{time},COALESCE(blocks,0)*8192,num_rows"),
            "tables",
            "table_name",
        ),
        (true, "view_name,'',0,0".into(), "views", "view_name"),
    ] {
        let (mut sql, bindings) = scope.statement(&columns, source, "");
        sql.push_str(" ORDER BY ");
        sql.push_str(order);
        let mut rows = query(&sql, &bindings).fetch(&mut *session);
        let mut fetched = 0;
        while let Some(row) = rows.try_next().await? {
            fetched += 1;
            ensure!(fetched <= 10_000, "对象超过 10,000 项，请缩小数据库范围");
            if !scope.accepts(&row)? {
                continue;
            }
            ensure!(
                catalog.objects.len() < 10_000,
                "对象超过 10,000 项，请缩小数据库范围"
            );
            let object = Object {
                table: TableId {
                    schema: scope.owner.into(),
                    name: scope.text(&row, 0)?,
                },
                view,
                comment: String::new(),
                engine: String::new(),
                auto_increment: String::new(),
                update_time: scope.text(&row, 1)?,
                data_length: data_length(scope.text(&row, 2)?.parse()?),
                rows: scope.text(&row, 3)?,
                fields: Vec::new(),
            };
            retained += object.table.schema.capacity()
                + object.table.name.capacity()
                + object.update_time.capacity()
                + object.data_length.capacity()
                + object.rows.capacity();
            let before = catalog.objects.capacity();
            catalog.objects.push(object);
            retained += (catalog.objects.capacity() - before) * std::mem::size_of::<Object>();
            check(retained, 0, budget)?;
        }
    }
    let mut names: Vec<_> = (0..catalog.objects.len()).collect();
    names.sort_unstable_by(|&a, &b| {
        let a = &catalog.objects[a];
        let b = &catalog.objects[b];
        a.view.cmp(&b.view).then(a.table.name.cmp(&b.table.name))
    });
    let scratch = names.capacity() * std::mem::size_of::<usize>();
    check(retained, scratch, budget)?;
    let (sql, bindings) = scope.statement(
        "table_name,table_type,comments",
        "tab_comments",
        "comments IS NOT NULL AND table_type IN ('TABLE','VIEW')",
    );
    let mut rows = query(&sql, &bindings).fetch(&mut *session);
    let mut fetched = 0;
    while let Some(row) = rows.try_next().await? {
        fetched += 1;
        ensure!(fetched <= 100_000, "对象注释超过 100,000 项");
        if !scope.accepts(&row)? {
            continue;
        }
        let name = scope.text(&row, 0)?;
        let view = match scope.text(&row, 1)?.as_str() {
            "TABLE" => false,
            "VIEW" => true,
            _ => continue,
        };
        let Ok(at) = names.binary_search_by(|&index| {
            let object = &catalog.objects[index];
            object
                .view
                .cmp(&view)
                .then(object.table.name.as_str().cmp(&name))
        }) else {
            continue;
        };
        let object = &mut catalog.objects[names[at]];
        let comment = scope.text(&row, 2)?;
        retained = retained
            .saturating_sub(object.comment.capacity())
            .saturating_add(comment.capacity());
        object.comment = comment;
        check(retained, scratch, budget)?;
    }
    Ok((catalog, retained))
}
