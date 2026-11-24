//! Native result strategies preserve text values, empty headers and physical origins.
use super::*;
use futures_util::{Stream, TryStreamExt};
use sqlx::{
    MySqlConnection, PgConnection, TypeInfo,
    mysql::{MySqlQueryResult, MySqlRow},
    postgres::{PgQueryResult, PgRow, PgValueFormat},
};
mod format;
mod metadata;

trait Protocol {
    type Row: Row;
    type Done;
    fn affected(done: &Self::Done) -> u64;
    fn row_columns(row: &Self::Row, result: &mut QueryResult) -> usize;
    fn done_columns(done: &Self::Done, result: &mut QueryResult) -> usize;
    fn cell(row: &Self::Row, index: usize) -> Result<Option<String>>;
}
struct MySql;
struct Postgres;
impl Protocol for MySql {
    type Row = MySqlRow;
    type Done = MySqlQueryResult;
    fn affected(done: &Self::Done) -> u64 {
        done.rows_affected()
    }
    fn row_columns(row: &Self::Row, result: &mut QueryResult) -> usize {
        metadata::mysql(row.columns(), result)
    }
    fn done_columns(done: &Self::Done, result: &mut QueryResult) -> usize {
        if done.has_result_set() {
            metadata::mysql(done.columns(), result)
        } else {
            0
        }
    }
    fn cell(row: &Self::Row, index: usize) -> Result<Option<String>> {
        if row.try_get_raw(index)?.is_null() {
            return Ok(None);
        }
        let bytes = row.try_get_unchecked::<Vec<u8>, _>(index)?;
        let kind = row.column(index).type_info().name();
        if kind == "BIT" {
            anyhow::ensure!(bytes.len() <= 8, "MySQL BIT 值超过 64 位");
            return Ok(Some(
                bytes
                    .iter()
                    .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
                    .to_string(),
            ));
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        };
        Ok(Some(match kind {
            "DECIMAL" => format::decimal(text),
            "DOUBLE" | "FLOAT" => format::double(text.parse()?),
            _ => text,
        }))
    }
}
impl Protocol for Postgres {
    type Row = PgRow;
    type Done = PgQueryResult;
    fn affected(done: &Self::Done) -> u64 {
        done.rows_affected()
    }
    fn row_columns(row: &Self::Row, result: &mut QueryResult) -> usize {
        metadata::postgres(row.columns(), result)
    }
    fn done_columns(done: &Self::Done, result: &mut QueryResult) -> usize {
        if done.has_result_set() {
            metadata::postgres(done.columns(), result)
        } else {
            0
        }
    }
    fn cell(row: &Self::Row, index: usize) -> Result<Option<String>> {
        let value = row.try_get_raw(index)?;
        if value.is_null() {
            return Ok(None);
        }
        anyhow::ensure!(
            value.format() == PgValueFormat::Text,
            "原始查询收到非文本 PostgreSQL 数据帧"
        );
        Ok(Some(row.try_get_unchecked::<String, _>(index)?))
    }
}
pub(super) async fn mysql(
    conn: &mut MySqlConnection,
    sql: &str,
    row_limit: usize,
) -> Result<Vec<QueryResult>> {
    collect::<MySql, _>(sqlx::raw_sql(sql).fetch_many(conn), row_limit).await
}
pub(super) async fn postgres(
    conn: &mut PgConnection,
    sql: &str,
    row_limit: usize,
) -> Result<Vec<QueryResult>> {
    let mut results =
        collect::<Postgres, _>(sqlx::raw_sql(sql).fetch_many(&mut *conn), row_limit).await?;
    // Catalog names are resolved after draining the user's result, on the same
    // session. Failure only removes editing eligibility, never a successful query.
    if let Err(error) = metadata::resolve_postgres(conn, &mut results).await {
        let message: String = format!("来源信息读取失败：{error:#}")
            .chars()
            .take(1024)
            .collect();
        for result in &mut results {
            if result
                .origins
                .iter()
                .any(|origin| origin.pg_reference.is_some() && origin.table.is_empty())
            {
                result.origin_error.get_or_insert_with(|| message.clone());
            }
        }
    }
    Ok(results)
}
fn charge(retained: &mut usize, bytes: usize) -> Result<()> {
    anyhow::ensure!(
        retained.saturating_add(bytes) <= limits::QUERY_BYTES,
        "查询列信息超过 8 MiB，请缩小结果范围"
    );
    *retained += bytes;
    Ok(())
}
async fn collect<P, S>(mut stream: S, row_limit: usize) -> Result<Vec<QueryResult>>
where
    P: Protocol,
    S: Stream<Item = std::result::Result<sqlx::Either<P::Done, P::Row>, sqlx::Error>> + Unpin,
{
    let mut results: Vec<QueryResult> = Vec::new();
    let mut current = QueryResult::default();
    let mut retained = 0usize;
    while let Some(item) = stream.try_next().await? {
        let keep = results.len() < limits::QUERY_RESULT_SETS;
        match item {
            sqlx::Either::Left(done) => {
                if keep && !current.has_result_set {
                    let bytes = P::done_columns(&done, &mut current);
                    charge(&mut retained, bytes)?;
                }
                current.affected = P::affected(&done);
                if keep {
                    results.push(std::mem::take(&mut current));
                } else {
                    current = QueryResult::default();
                    if let Some(last) = results.last_mut() {
                        last.truncated = true;
                    }
                }
            }
            sqlx::Either::Right(row) => {
                if keep && !current.has_result_set {
                    let bytes = P::row_columns(&row, &mut current);
                    charge(&mut retained, bytes)?;
                }
                current.has_result_set = true;
                current.total_rows = current.total_rows.saturating_add(1);
                if keep && current.rows.len() < row_limit && retained < limits::QUERY_BYTES {
                    let values = (0..row.len())
                        .map(|index| P::cell(&row, index))
                        .collect::<Result<Vec<_>>>()?;
                    let bytes = current.row_bytes(&values);
                    if retained.saturating_add(bytes) <= limits::QUERY_BYTES {
                        retained += bytes;
                        current.push_row(values);
                    } else {
                        current.truncated = true;
                    }
                } else {
                    current.truncated = true;
                }
            }
        }
    }
    if current.has_result_set && results.len() < limits::QUERY_RESULT_SETS {
        results.push(current);
    }
    Ok(results)
}
