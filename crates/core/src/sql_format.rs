//! Formatting behavior of SqlEditorView.formatSql, including its case-sensitive substitutions.
use std::sync::LazyLock;
static SPACE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[ \t\r\n\x0b\x0c]+").unwrap());
static BLANK: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\n[ \t\r\n\x0b\x0c]*\n").unwrap());
pub fn format(text: &str) -> anyhow::Result<String> {
    anyhow::ensure!(text.len() <= 8 * 1024 * 1024, "SQL 正文超过 8 MiB");
    let mut sql = SPACE
        .replace_all(text, " ")
        .trim_matches(|ch: char| ch <= ' ')
        .to_owned();
    for keyword in [
        "SELECT",
        "FROM",
        "WHERE",
        "INNER JOIN",
        "LEFT JOIN",
        "RIGHT JOIN",
        "CROSS JOIN",
        "FULL JOIN",
        "ON",
        "GROUP BY",
        "ORDER BY",
        "HAVING",
        "LIMIT",
        "OFFSET",
        "UNION",
        "INSERT INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE FROM",
        "CREATE TABLE",
        "DROP TABLE",
        "ALTER TABLE",
    ] {
        for keyword in [
            keyword.to_owned(),
            keyword.to_owned(),
            keyword.to_lowercase(),
        ] {
            sql = sql.replace(&format!(" {keyword} "), &format!("\n{keyword}\n    "));
            anyhow::ensure!(
                sql.len() <= 8 * 1024 * 1024,
                "格式化后的 SQL 超过 8 MiB，保留原文"
            );
        }
    }
    let result = BLANK.replace_all(&sql, "\n");
    let result = result.trim_matches(|ch: char| ch <= ' ');
    Ok(result.strip_prefix("    ").unwrap_or(result).to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Deserialize)]
    struct Case {
        input: String,
        output: String,
    }
    #[test]
    fn formatter_matches_twenty_actual_java_outputs() -> anyhow::Result<()> {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../tests/fixtures/sql-format/original.json"))?;
        assert_eq!(cases.len(), 20);
        for (index, case) in cases.into_iter().enumerate() {
            assert_eq!(
                format(&case.input)?,
                case.output,
                "Java formatter case {index}"
            );
        }
        Ok(())
    }
}
