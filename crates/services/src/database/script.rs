//! User scripts execute statement-by-statement; metadata queries retain the raw port.
use super::*;
mod interrupted;
pub use interrupted::{Interrupted, Interruption, Phase};
use serde::{Deserialize, Serialize};
use std::{ops::Range, time::Instant};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Run,
    Explain,
}
#[derive(Debug, Serialize)]
pub struct Statement {
    pub range: Range<usize>,
    pub select: bool,
    pub source_table: Option<String>,
    pub elapsed_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub result: Option<usize>,
    pub has_result_set: bool,
    pub row_count: u64,
    pub affected: Option<u64>,
    pub truncated: bool,
    pub editing: Option<super::query_rows::Editing>,
    pub edit_error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Plan {
    pub range: Range<usize>,
    pub result: usize,
}
#[derive(Debug, Default, Serialize)]
pub struct Script {
    pub source: String,
    pub mode: Mode,
    pub statements: Vec<Statement>,
    pub results: Vec<QueryResult>,
    pub plans: Vec<Plan>,
    pub status: Option<usize>,
    pub elapsed_ms: u64,
    pub success_count: u64,
    pub fail_count: u64,
    pub metadata_truncated: bool,
    pub omitted_results: u64,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interruption: Option<Interruption>,
}
impl Script {
    pub fn sql(&self, range: &Range<usize>) -> &str {
        self.source.get(range.clone()).unwrap_or_default()
    }
    pub fn row_results(&self) -> impl Iterator<Item = (usize, &Statement)> {
        self.statements
            .iter()
            .enumerate()
            .filter(|(_, statement)| statement.success && statement.result.is_some())
    }
    pub fn info(&self) -> String {
        use std::fmt::Write;
        let mut text = String::new();
        for (index, statement) in self.statements.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(self.sql(&statement.range));
            text.push('\n');
            if statement.success {
                let _ = writeln!(
                    text,
                    " > OK\n > 时间: {}.{:03}s",
                    statement.elapsed_ms / 1000,
                    statement.elapsed_ms % 1000
                );
                if statement.has_result_set {
                    let _ = writeln!(text, " > 行数: {}", statement.row_count);
                } else if let Some(affected) = statement.affected {
                    let _ = writeln!(text, " > 影响: {affected} 行");
                }
            } else {
                text.push_str(" > 错误\n > ");
                text.push_str(statement.error.as_deref().unwrap_or_default());
                text.push('\n');
            }
        }
        let _ = write!(
            text,
            "\n--- 汇总 ---\n总耗时: {}.{:03}s\n成功: {}  失败: {}",
            self.elapsed_ms / 1000,
            self.elapsed_ms % 1000,
            self.success_count,
            self.fail_count
        );
        if self.metadata_truncated {
            text.push_str(
                "\n执行信息超过保留预算，以上仅显示已保留的语句；成功/失败统计包含全部语句。",
            );
        }
        if self.omitted_results > 0 {
            let _ = write!(
                text,
                "\n{} 个结果超过保留预算，未在内存中保留。",
                self.omitted_results
            );
        }
        if let Some(interruption) = &self.interruption {
            text.push_str("\n\n");
            text.push_str(&interruption.message);
        }
        text
    }
    fn push_statement(&mut self, record: Statement, metadata_bytes: &mut usize) {
        let bytes = std::mem::size_of::<Statement>()
            + record.source_table.as_ref().map_or(0, String::capacity)
            + record.error.as_ref().map_or(0, String::capacity);
        if self.statements.len() < 10000 && metadata_bytes.saturating_add(bytes) <= 8 * 1024 * 1024
        {
            *metadata_bytes += bytes;
            self.statements.push(record);
        } else {
            self.metadata_truncated = true;
        }
    }
    fn keep(&mut self, result: QueryResult, retained: &mut usize) -> Option<usize> {
        if self.results.len() >= limits::QUERY_RESULT_SETS {
            self.omitted_results += 1;
            return None;
        }
        let remaining = limits::QUERY_BYTES.saturating_sub(*retained);
        let mut result = result;
        if result.retained_bytes() > remaining {
            result.truncated = true;
            let original = std::mem::take(&mut result.rows);
            let mut bytes = result.retained_bytes();
            let mut full = false;
            for row in original {
                let needed = row.iter().fold(
                    std::mem::size_of::<Vec<String>>()
                        + row.capacity() * std::mem::size_of::<String>(),
                    |sum, value| sum.saturating_add(value.capacity()),
                );
                if !full && bytes.saturating_add(needed) <= remaining {
                    bytes += needed;
                    result.rows.push(row);
                } else {
                    full = true;
                }
            }
            result.rows.shrink_to_fit();
            result
                .null_cells
                .truncate((result.rows.len() * result.columns.len()).div_ceil(64));
            result.null_cells.shrink_to_fit();
            if result.retained_bytes() > remaining {
                self.omitted_results += 1;
                return None;
            }
        }
        *retained = retained.saturating_add(result.retained_bytes());
        let index = self.results.len();
        self.results.push(result);
        Some(index)
    }
}
fn elapsed(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}
fn error_text(error: &anyhow::Error) -> String {
    format!("{error:#}").chars().take(4096).collect()
}
fn error_result(error: &anyhow::Error) -> QueryResult {
    QueryResult {
        columns: vec!["错误".into()],
        rows: vec![vec![error_text(error)]],
        has_result_set: true,
        total_rows: 1,
        column_types: vec![12],
        ..Default::default()
    }
}
impl Databases {
    async fn script_literal_mode(&mut self, connection: &Connection, retry: bool) -> Result<bool> {
        let sql = match connection.kind() {
            "MYSQL" => "SELECT @@SESSION.sql_mode",
            "POSTGRESQL" => "SHOW standard_conforming_strings",
            _ => return Ok(false),
        };
        let results = match self.query(connection, sql).await {
            Err(error)
                if retry
                    && error
                        .downcast_ref::<super::ConnectionFailure>()
                        .is_some_and(|failure| {
                            !failure.transaction_lost
                                && failure.stage == super::FailureStage::Executing
                                && matches!(
                                    failure.recovery,
                                    super::RecoveryState::Restored
                                        | super::RecoveryState::Disconnected
                                )
                        }) =>
            {
                self.query(connection, sql).await?
            }
            other => other?,
        };
        let value = results
            .first()
            .and_then(|r| r.rows.first())
            .and_then(|r| r.first())
            .ok_or_else(|| anyhow::anyhow!("无法读取会话字符串模式"))?;
        Ok(if connection.kind() == "MYSQL" {
            !value
                .split(',')
                .any(|part| part.eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
        } else {
            value.eq_ignore_ascii_case("off")
        })
    }
    async fn explain_one(&mut self, connection: &Connection, sql: &str) -> Result<QueryResult> {
        match self.query(connection, &format!("EXPLAIN {sql}")).await {
            Ok(results) => Ok(results.into_iter().next().unwrap_or_default()),
            Err(error) if error.downcast_ref::<super::ConnectionFailure>().is_some() => {
                Err(error.context("SQL 执行后的计划读取连接中断；请核对已执行语句的结果"))
            }
            Err(error) => Ok(error_result(&error)),
        }
    }
    pub async fn script(
        &mut self,
        connection: &Connection,
        sql: String,
        mode: Mode,
    ) -> Result<Script> {
        anyhow::ensure!(sql.len() <= 8 * 1024 * 1024, "SQL 正文超过 8 MiB");
        let mut output = Script {
            mode,
            ..Default::default()
        };
        let start = Instant::now();
        let base = sql.as_ptr() as usize;
        if tomato_core::sql_script::split_for(connection.kind(), Some(&sql))
            .next()
            .is_none()
        {
            output.source = sql;
            return Ok(output);
        }
        // Reject non-query EXPLAIN locally, before reading any session settings.
        // Either literal mode may expose a SELECT, so only skip when neither does.
        if matches!(mode, Mode::Explain)
            && ![false, true].into_iter().any(|backslash| {
                tomato_core::sql_script::split_for_session(connection.kind(), Some(&sql), backslash)
                    .any(|statement| tomato_core::sql_script::is_select(Some(statement)))
            })
        {
            output.message = Some("没有可解释的SELECT语句".into());
            output.source = sql;
            return Ok(output);
        }
        let backslash = self.script_literal_mode(connection, true).await?;
        let statements =
            || tomato_core::sql_script::split_for_session(connection.kind(), Some(&sql), backslash);
        if matches!(mode, Mode::Explain)
            && !statements().any(|statement| tomato_core::sql_script::is_select(Some(statement)))
        {
            output.message = Some("没有可解释的SELECT语句".into());
            output.source = sql;
            return Ok(output);
        }
        // Acquire before the stream; a failed opening must not retry per remaining statement.
        self.session(connection).await?;
        let mut retained = 0usize;
        let mut metadata_bytes = 0usize;
        if matches!(mode, Mode::Run) {
            let mut stream = statements();
            let mut literal_mode = backslash;
            while let Some(statement) = stream.next() {
                let from = statement.as_ptr() as usize - base;
                let range = from..from + statement.len();
                let select = tomato_core::sql_script::is_select(Some(statement));
                let target = if select {
                    tomato_core::sql_script::source_table(Some(statement))
                        .ok()
                        .flatten()
                        .filter(|name| name.len() <= 1024)
                } else {
                    None
                };
                let started = Instant::now();
                let mut record = Statement {
                    range,
                    select,
                    source_table: target,
                    elapsed_ms: 0,
                    success: false,
                    error: None,
                    result: None,
                    has_result_set: false,
                    row_count: 0,
                    affected: None,
                    truncated: false,
                    editing: None,
                    edit_error: None,
                };
                match self
                    .query_without_restore(connection, statement, Some(literal_mode))
                    .await
                {
                    Ok(results) => {
                        record.success = true;
                        output.success_count = output.success_count.saturating_add(1);
                        if let Some(result) = results.into_iter().next() {
                            if result.is_rowset() {
                                record.has_result_set = true;
                                record.row_count = result.row_count();
                                record.truncated = result.truncated;
                                record.result = output.keep(result, &mut retained);
                                record.truncated |= record
                                    .result
                                    .is_none_or(|index| output.results[index].truncated);
                            } else {
                                record.affected = Some(result.affected);
                            }
                        }
                    }
                    Err(error) => {
                        if error.downcast_ref::<super::ConnectionFailure>().is_some() {
                            output.fail_count = output.fail_count.saturating_add(1);
                            record.error = Some(error_text(&error));
                            record.elapsed_ms = elapsed(started);
                            output.push_statement(record, &mut metadata_bytes);
                            let stopped_elapsed = elapsed(start);
                            let error = self
                                .restore_after_error(connection, connection.text("database"), error)
                                .await;
                            return Err(interrupted::interrupt(
                                output,
                                sql,
                                error,
                                Phase::Statement,
                                stopped_elapsed,
                            ));
                        }
                        output.fail_count = output.fail_count.saturating_add(1);
                        record.error = Some(error_text(&error));
                    }
                }
                if record.success
                    && [
                        b"SQL_MODE".as_slice(),
                        b"STANDARD_CONFORMING_STRINGS".as_slice(),
                    ]
                    .iter()
                    .any(|word| {
                        statement
                            .as_bytes()
                            .windows(word.len())
                            .any(|part| part.eq_ignore_ascii_case(word))
                    })
                {
                    let mode = match self.script_literal_mode(connection, false).await {
                        Ok(mode) => mode,
                        Err(error) => {
                            record.elapsed_ms = elapsed(started);
                            output.push_statement(record, &mut metadata_bytes);
                            return Err(interrupted::interrupt(
                                output,
                                sql,
                                error,
                                Phase::SessionSettings,
                                elapsed(start),
                            ));
                        }
                    };
                    literal_mode = mode;
                    stream.set_backslash(mode);
                }
                record.elapsed_ms = elapsed(started);
                output.push_statement(record, &mut metadata_bytes);
            }
            // Original app obtains plans only for successful SELECT-class rowsets.
            output.elapsed_ms = elapsed(start);
            let plans: Vec<_> = output
                .statements
                .iter()
                .filter(|statement| {
                    statement.success && statement.select && statement.result.is_some()
                })
                .map(|statement| statement.range.clone())
                .collect();
            for range in plans {
                let result = match self.explain_one(connection, &sql[range.clone()]).await {
                    Ok(result) => result,
                    Err(error) => {
                        return Err(interrupted::interrupt(
                            output,
                            sql,
                            error,
                            Phase::Plan,
                            elapsed(start),
                        ));
                    }
                };
                if let Some(index) = output.keep(result, &mut retained) {
                    output.plans.push(Plan {
                        range,
                        result: index,
                    });
                }
            }
            let status_sql = match connection.kind() {
                "MYSQL" => Some("SHOW STATUS"),
                "POSTGRESQL" => Some(
                    "SELECT name, setting, short_desc AS \"Description\", category FROM pg_settings ORDER BY category, name",
                ),
                _ => None,
            };
            let status = if let Some(status_sql) = status_sql {
                let response = match self.query(connection, status_sql).await {
                    Err(error) if error.downcast_ref::<super::ConnectionFailure>().is_some() => {
                        return Err(interrupted::interrupt(
                            output,
                            sql,
                            error,
                            Phase::Status,
                            elapsed(start),
                        ));
                    }
                    other => other,
                };
                response.ok().and_then(|results| results.into_iter().next())
            } else {
                Some(QueryResult {
                    columns: vec!["信息".into()],
                    rows: vec![vec!["当前数据库类型不支持状态查询".into()]],
                    has_result_set: true,
                    total_rows: 1,
                    ..Default::default()
                })
            };
            if let Some(status) = status {
                output.status = output.keep(status, &mut retained);
            }
        } else {
            let stream = statements()
                .filter(|statement| tomato_core::sql_script::is_select(Some(statement)));
            for statement in stream {
                let from = statement.as_ptr() as usize - base;
                let range = from..from + statement.len();
                let result = match self.explain_one(connection, statement).await {
                    Ok(result) => result,
                    Err(error) => {
                        return Err(interrupted::interrupt(
                            output,
                            sql,
                            error,
                            Phase::Plan,
                            elapsed(start),
                        ));
                    }
                };
                if let Some(index) = output.keep(result, &mut retained) {
                    output.plans.push(Plan {
                        range,
                        result: index,
                    });
                }
            }
        }
        if matches!(mode, Mode::Explain) {
            output.elapsed_ms = elapsed(start);
        }
        if matches!(mode, Mode::Run) {
            let mut editing_bytes = 0usize;
            for index in 0..output.statements.len() {
                let statement = &output.statements[index];
                if let (Some(target), Some(result)) =
                    (statement.source_table.as_deref(), statement.result)
                {
                    match self
                        .query_editing(connection, target, &output.results[result])
                        .await
                    {
                        Ok(Some(editing)) => {
                            let bytes = editing.retained_bytes();
                            if editing_bytes.saturating_add(bytes) <= 256 * 1024 {
                                editing_bytes += bytes;
                                output.statements[index].editing = Some(editing);
                            } else {
                                output.statements[index].edit_error =
                                    Some("结果编辑元信息超过 256 KiB，请缩小查询范围".into());
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            output.statements[index].edit_error = Some(error_text(&error));
                            if error.downcast_ref::<super::ConnectionFailure>().is_some() {
                                return Err(interrupted::interrupt(
                                    output,
                                    sql,
                                    error,
                                    Phase::EditingMetadata,
                                    elapsed(start),
                                ));
                            }
                        }
                    }
                }
            }
        }
        output.source = sql;
        Ok(output)
    }
}
