//! Owned partial output; error diagnostics never format SQL text or row buffers.
use super::*;
use crate::database::ConnectionFailure;
use std::fmt;
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Statement,
    SessionSettings,
    Plan,
    Status,
    EditingMetadata,
}
#[derive(Debug, Serialize)]
pub struct Interruption {
    pub phase: Phase,
    pub connection: Option<ConnectionFailure>,
    pub message: String,
}
pub struct Interrupted {
    pub script: Box<Script>,
}
impl fmt::Debug for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptedScript")
            .field("success_count", &self.script.success_count)
            .field("fail_count", &self.script.fail_count)
            .field("retained_results", &self.script.results.len())
            .finish_non_exhaustive()
    }
}
impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SQL 脚本已停止，之前有 {} 条语句执行成功，查询结果已保留",
            self.script.success_count
        )
    }
}
impl std::error::Error for Interrupted {}
pub(super) fn interrupt(
    mut output: Script,
    source: String,
    error: anyhow::Error,
    phase: Phase,
    elapsed_ms: u64,
) -> anyhow::Error {
    let connection = error.downcast_ref::<ConnectionFailure>().cloned();
    let reason = connection
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{error:#}").chars().take(4096).collect());
    let message = format!(
        "{}\n{}",
        match phase {
            Phase::Statement => "后续 SQL 语句已停止，之前的查询结果已保留。",
            Phase::SessionSettings => "读取会话设置失败，后续 SQL 语句已停止，已执行的结果已保留。",
            Phase::Plan => "SQL 已执行，执行计划读取失败；查询结果已保留。",
            Phase::Status => "SQL 已执行，附加状态读取失败；查询结果已保留。",
            Phase::EditingMetadata => "查询结果已保留；源表结构读取失败，暂时无法编辑这些结果。",
        },
        reason
    );
    output.source = source;
    if matches!(phase, Phase::Statement | Phase::SessionSettings)
        || matches!(output.mode, Mode::Explain)
    {
        output.elapsed_ms = elapsed_ms;
    }
    output.interruption = Some(Interruption {
        phase,
        connection,
        message,
    });
    error.context(Interrupted {
        script: Box::new(output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interruption_moves_buffers_and_diagnostics_never_dump_sql_or_rows() {
        let source = String::from("SELECT 'private-sql-literal'");
        let source_ptr = source.as_ptr();
        let result = QueryResult {
            rows: vec![vec![String::from("private-row-buffer")]],
            ..Default::default()
        };
        let row_ptr = result.rows[0][0].as_ptr();
        let error = interrupt(
            Script {
                results: vec![result],
                success_count: 1,
                ..Default::default()
            },
            source,
            anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "owned fixture",
            )),
            Phase::Statement,
            41,
        );
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("private-row-buffer"));
        assert!(!diagnostic.contains("private-sql-literal"));
        let interrupted = error.downcast::<Interrupted>().unwrap();
        assert_eq!(interrupted.script.source.as_ptr(), source_ptr);
        assert_eq!(interrupted.script.results[0].rows[0][0].as_ptr(), row_ptr);
        assert_eq!(interrupted.script.elapsed_ms, 41);
    }
}
