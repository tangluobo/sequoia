//! Stream one source row at a time; destination transactions contain at most 256 rows.
use super::*;
use tomato_core::workbench::TableDefinition;
fn select(kind: &str, definition: &TableDefinition) -> Result<String> {
    let columns = definition
        .columns
        .iter()
        .map(|column| workbench::projection(kind, column))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!(
        "SELECT {} FROM {}",
        columns.join(","),
        workbench::plan::qualified(kind, &definition.table)?
    ))
}
pub(super) async fn copy(
    source: &mut AnyConnection,
    target: &mut AnyConnection,
    request: &Request,
    table: &TablePlan,
    control: &Control,
) -> Result<u64> {
    let before = control
        .read(|progress| progress.records)
        .unwrap_or_default();
    let identity = if request.target.kind() == "ORACLE" {
        Some(super::super::oracle::identity::Identity::prepare(target, &table.target).await?)
    } else {
        None
    };
    let outcome = copy_rows(source, target, request, table, control).await;
    if let Some(identity) = identity {
        let reset = control
            .read(|progress| progress.records)
            .unwrap_or_default()
            > before
            || outcome.as_ref().is_ok_and(|count| *count > 0);
        let cleanup = identity.finish(target, reset).await;
        if let Err(error) = cleanup {
            return match outcome {
                Ok(_) => Err(error),
                Err(original) => Err(original.context(format!("恢复 Oracle 标识列失败：{error}"))),
            };
        }
    }
    outcome
}
async fn copy_rows(
    source: &mut AnyConnection,
    target: &mut AnyConnection,
    request: &Request,
    table: &TablePlan,
    control: &Control,
) -> Result<u64> {
    let original = workbench::describe(source, request.source.kind(), &table.source).await?;
    let destination = workbench::describe(target, request.target.kind(), &table.target).await?;
    let transactional = destination.transactional;
    if !transactional {
        control.log("目标表引擎不支持事务，取消或失败时已写入的记录会保留");
    }
    let mut immediate = Immediate {
        control,
        pending: 0,
    };
    // One prepared INSERT for the entire table, including all NULL combinations.
    // Extra destination columns retain their defaults; generated columns are never assigned.
    let mut mapping = Vec::new();
    let mut names = Vec::new();
    let mut arguments = Vec::new();
    let dialect = crate::dialect::resolve(request.target.kind())?;
    for (index, column) in original.columns.iter().enumerate() {
        let (target_index, target_column) = destination
            .columns
            .iter()
            .enumerate()
            .find(|(_, c)| c.name == column.name)
            .with_context(|| format!("目标表缺少源字段 {}", column.name))?;
        if target_column.generated {
            continue;
        }
        mapping.push((index, target_index));
        names.push(dialect.quote(&column.name)?);
        let parameter = workbench::plan::placeholder(request.target.kind(), mapping.len());
        arguments.push(if request.target.kind() == "POSTGRESQL" {
            format!(
                "CAST({parameter} AS {})",
                workbench::plan::qualified(
                    "POSTGRESQL",
                    &TableId {
                        schema: target_column.type_schema.clone(),
                        name: target_column.type_name.clone()
                    }
                )?
            )
        } else if request.target.kind() == "MYSQL" && workbench::plan::mysql_bit(target_column) {
            format!("CAST(CAST({parameter} AS CHAR) AS UNSIGNED)")
        } else if request.target.kind() == "ORACLE" {
            workbench::plan::oracle_parameter(target_column, &parameter)
        } else {
            parameter
        });
    }
    ensure!(!mapping.is_empty(), "没有可传输的普通字段");
    let qualified = workbench::plan::qualified(request.target.kind(), &destination.table)?;
    let override_identity = if request.target.kind() == "POSTGRESQL"
        && destination.columns.iter().any(|c| c.auto_increment)
    {
        " OVERRIDING SYSTEM VALUE"
    } else {
        ""
    };
    let insert = format!(
        "INSERT INTO {qualified} ({}){override_identity} VALUES ({})",
        names.join(","),
        arguments.join(",")
    );
    let oracle_prefix = format!("INSERT INTO {qualified} ({}) VALUES (", names.join(","));
    let sql = select(request.source.kind(), &original)?;
    let _budget = source
        .backend_mut::<super::super::oracle::Backend>()
        .map(|backend| backend.streaming());
    let mut stream = sqlx::query(&sql).fetch(source);
    let mut transaction = target.begin().await?;
    let mut pending = 0u64;
    let mut count = 0u64;
    while let Some(row) = tokio::select! { result=stream.try_next()=>result?, ()=control.wait_cancelled()=>{anyhow::bail!("操作已取消；已提交{count}行")} }
    {
        control.check()?;
        let values = mapping
            .iter()
            .map(|(index, target_index)| {
                let mut value = if request.source.kind() == "ORACLE" {
                    workbench::native_cell(&row, *index)?
                } else {
                    workbench::strings(&row, *index)?
                };
                if let Cell::Text(text) = &value {
                    ensure!(text.len() <= BUDGET, "单个传输字段超过8 MiB");
                    if original.columns[*index].binary {
                        value = Cell::Binary(workbench::unhex(text)?);
                    } else if matches!(request.target.kind(), "MYSQL" | "ORACLE")
                        && matches!(
                            original.columns[*index].data_type.as_str(),
                            "boolean" | "bool"
                        )
                    {
                        value = Cell::Integer(i64::from(text == "true" || text == "t"));
                    } else if request.target.kind() == "POSTGRESQL"
                        && destination.columns[*target_index].type_name == "bool"
                        && text == "0"
                    {
                        value = Cell::Text("false".into());
                    }
                }
                if request.target.kind() == "MYSQL"
                    && workbench::plan::mysql_bit(&destination.columns[*target_index])
                {
                    value = workbench::plan::mysql_bit_value(
                        &destination.columns[*target_index],
                        &value,
                    )?;
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values
                .iter()
                .map(|v| match v {
                    Cell::Text(s) => s.len(),
                    Cell::Binary(b) => b.len(),
                    _ => 8,
                })
                .sum::<usize>()
                <= BUDGET,
            "单行传输数据超过8 MiB"
        );
        let (statement, parameters) = if request.target.kind() == "ORACLE" {
            let mut parameters = Vec::with_capacity(values.len());
            let arguments = mapping
                .iter()
                .zip(&values)
                .map(|((_, index), value)| {
                    workbench::plan::argument(
                        "ORACLE",
                        &destination.columns[*index],
                        value,
                        &mut parameters,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            drop(values);
            (
                std::borrow::Cow::Owned(format!("{oracle_prefix}{})", arguments.join(","))),
                parameters,
            )
        } else {
            (std::borrow::Cow::Borrowed(insert.as_str()), values)
        };
        tokio::select! {result=bind(&statement,parameters).execute(&mut *transaction)=>{result.with_context(||format!("写入源第{}行失败；已提交{count}行",count+pending+1))?;},()=control.wait_cancelled()=>{anyhow::bail!("操作已取消；已提交{count}行");}}
        pending += 1;
        if !transactional {
            immediate.pending += 1;
            if immediate.pending == 256 {
                control.records(immediate.pending);
                immediate.pending = 0;
            }
        }
        if pending == 256 {
            transaction.commit().await?;
            count += pending;
            if transactional {
                control.records(pending);
            }
            pending = 0;
            transaction = target.begin().await?;
        }
    }
    control.check()?;
    transaction.commit().await?;
    count += pending;
    if transactional {
        control.records(pending);
    }
    if request.target.kind() == "POSTGRESQL" && count > 0 {
        super::postgres::reset_identity(target, &destination).await?;
    }
    Ok(count)
}

struct Immediate<'a> {
    control: &'a Control,
    pending: u64,
}
impl Drop for Immediate<'_> {
    fn drop(&mut self) {
        if self.pending > 0 {
            self.control.records(self.pending);
        }
    }
}

fn bind(
    sql: &str,
    values: Vec<Cell>,
) -> sqlx::query::Query<'_, sqlx::Any, sqlx::any::AnyArguments<'_>> {
    let mut query = sqlx::query(sql);
    for value in values {
        query = match value {
            Cell::Null => query.bind(Option::<String>::None),
            Cell::Text(value) => query.bind(value),
            Cell::Integer(value) => query.bind(value),
            Cell::Real(value) => query.bind(value),
            Cell::Boolean(value) => query.bind(value),
            Cell::Binary(value) => query.bind(value),
        };
    }
    query
}
