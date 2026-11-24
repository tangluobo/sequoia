//! Object operations share the SQL session. Successful siblings survive a failed target.
use super::*;
use sqlx::Executor;
use tomato_core::database_objects::{ChangeReport, Mutation, Request, Target};

fn plan(kind: &str, target: &Target, mutation: &Mutation) -> Result<Option<String>> {
    ensure!(
        matches!(kind, "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"),
        "当前数据库尚未实现对象操作"
    );
    let dialect = crate::dialect::resolve(kind)?;
    let qualified = format!(
        "{}.{}",
        dialect.quote(&target.table.schema)?,
        dialect.quote(&target.table.name)?
    );
    let object = if target.view { "VIEW" } else { "TABLE" };
    Ok(Some(match mutation {
        Mutation::Drop => format!("DROP {object} {qualified}"),
        Mutation::Clear if !target.view => format!("DELETE FROM {qualified}"),
        Mutation::Truncate if !target.view => format!("TRUNCATE TABLE {qualified}"),
        Mutation::Rename { name } => {
            ensure!(name.len() <= 4096, "对象名称过长");
            let name = dialect.quote(name)?;
            if kind == "MYSQL" {
                format!(
                    "RENAME TABLE {qualified} TO {}.{name}",
                    dialect.quote(&target.table.schema)?
                )
            } else if kind == "ORACLE" && target.view {
                // RENAME only operates on the login owner's schema. The
                // execution guard below prevents renaming a same-named view
                // in that schema when a different owner's view was selected.
                format!("RENAME {} TO {name}", dialect.quote(&target.table.name)?)
            } else {
                format!("ALTER {object} {qualified} RENAME TO {name}")
            }
        }
        _ => return Ok(None),
    }))
}

pub(in crate::database) async fn execute(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
    request: Request,
) -> Result<ChangeReport> {
    ensure!(
        !request.targets.is_empty() && request.targets.len() <= 1000,
        "每次请选择 1 至 1,000 个对象"
    );
    ensure!(
        !matches!(&request.mutation, Mutation::Rename { .. }) || request.targets.len() == 1,
        "重命名只能选择一个对象"
    );
    let mut unique = std::collections::HashSet::with_capacity(request.targets.len());
    let mut bytes = 0usize;
    let plans = request
        .targets
        .iter()
        .map(|target| {
            ensure!(target.table.schema == schema, "对象不属于当前数据库或模式");
            ensure!(unique.insert(&target.table.name), "不能重复操作同一对象");
            let sql = plan(kind, target, &request.mutation)?;
            bytes = bytes.saturating_add(sql.as_ref().map_or(0, String::capacity));
            ensure!(bytes <= 1024 * 1024, "对象操作语句超过 1 MiB");
            Ok(sql)
        })
        .collect::<Result<Vec<_>>>()?;
    let point = workbench::savepoint_name(kind, "objects");
    if kind == "ORACLE"
        && matches!(&request.mutation, Mutation::Rename { .. })
        && request.targets.iter().any(|t| t.view)
    {
        let row = sqlx::query("SELECT USER FROM dual")
            .fetch_one(&mut *session)
            .await?;
        ensure!(
            workbench::text(&row, 0)? == schema,
            "Oracle 视图重命名必须使用对象所属用户连接"
        );
    }
    let transaction = workbench::existing_savepoint(session, kind, &point).await?;
    if transaction {
        workbench::release_savepoint(session, kind, &point).await?;
        // MySQL DDL commits implicitly. Preserve the user's SQL transaction.
        ensure!(
            !matches!(kind, "MYSQL" | "ORACLE") || matches!(&request.mutation, Mutation::Clear),
            "当前存在未完成事务，请先提交或回滚再修改数据库对象"
        );
    }
    let mut report = ChangeReport::default();
    for (target, sql) in request.targets.into_iter().zip(plans) {
        let Some(sql) = sql else {
            continue;
        };
        if transaction {
            session
                .execute(format!("SAVEPOINT {point}").as_str())
                .await?;
        }
        match session.execute(sql.as_str()).await {
            Ok(done) => {
                report.affected = report.affected.saturating_add(done.rows_affected());
                report.succeeded.push(target.table);
            }
            Err(error) => {
                if transaction {
                    session
                        .execute(format!("ROLLBACK TO SAVEPOINT {point}").as_str())
                        .await?;
                }
                let mut message = error.to_string();
                if message.len() > 4096 {
                    let mut end = 4096;
                    while !message.is_char_boundary(end) {
                        end -= 1;
                    }
                    message.truncate(end);
                }
                report.failures.push((target.table, message));
            }
        }
        if transaction {
            workbench::release_savepoint(session, kind, &point).await?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_quote_each_identifier_and_do_not_clear_or_truncate_views() -> Result<()> {
        let target = Target {
            table: TableId {
                schema: "odd`\"schema".into(),
                name: "table`\"; DROP TABLE users; --".into(),
            },
            view: false,
        };
        for kind in ["MYSQL", "POSTGRESQL"] {
            let sql = plan(
                kind,
                &target,
                &Mutation::Rename {
                    name: "new`\"name".into(),
                },
            )?
            .unwrap();
            let dialect = crate::dialect::resolve(kind)?;
            assert!(sql.contains(&dialect.quote(&target.table.name)?));
            assert!(sql.contains(&dialect.quote("new`\"name")?));
            let view = Target {
                view: true,
                ..target.clone()
            };
            assert!(plan(kind, &view, &Mutation::Clear)?.is_none());
            assert!(plan(kind, &view, &Mutation::Truncate)?.is_none());
            assert!(
                plan(kind, &view, &Mutation::Drop)?
                    .unwrap()
                    .starts_with("DROP VIEW ")
            );
            assert!(plan(kind, &target, &Mutation::Rename { name: "\0".into() }).is_err());
        }
        Ok(())
    }
}
