//! Original AI SQL text/protocol policy. Network and document writes are separate.
use anyhow::{Result, ensure};
use serde_json::Value;
pub mod editor;
pub mod settings;
pub const MAX_TEXT: usize = 8 * 1024 * 1024;
fn trim(text: &str) -> &str {
    text.trim_matches(|ch| ch <= ' ')
}
fn blank(text: &str) -> bool {
    crate::java_text::is_blank(text)
}
pub fn is_blank(text: &str) -> bool {
    blank(text)
}
fn ascii_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t'..='\r')
}
fn fold(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    let first = upper.next().unwrap_or(ch);
    let upper = if upper.next().is_none() { first } else { ch };
    upper.to_lowercase().next().unwrap_or(upper)
}
pub fn endpoint(base: Option<&str>) -> String {
    let value = trim(base.unwrap_or_default()).trim_end_matches('/');
    let suffix = "/chat/completions";
    if value.len() >= suffix.len()
        && value
            .get(value.len() - suffix.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
    {
        value.into()
    } else {
        format!("{value}{suffix}")
    }
}
pub fn clean(content: Option<&str>) -> &str {
    let mut value = trim(content.unwrap_or_default());
    if value.starts_with("```")
        && value.ends_with("```")
        && let Some(newline) = value.find('\n')
    {
        value = trim(&value[newline + 1..value.len() - 3]);
    }
    let end = value
        .char_indices()
        .nth(4)
        .map_or(value.len(), |(index, _)| index);
    if value[..end].chars().map(fold).eq("sql:".chars()) {
        value = trim(&value[end..]);
    }
    value
}
pub fn looks_sql(mut text: &str) -> bool {
    text = text.trim_start_matches(ascii_space);
    loop {
        if let Some(comment) = text.strip_prefix("--") {
            let Some(index) = comment.find(['\r', '\n']) else {
                return false;
            };
            let newline = if comment.as_bytes()[index] == b'\r' {
                if comment.as_bytes().get(index + 1) != Some(&b'\n') {
                    return false;
                }
                index + 2
            } else {
                index + 1
            };
            text = &comment[newline..];
        } else if let Some(comment) = text.strip_prefix("/*") {
            let Some(end) = comment.find("*/") else {
                return false;
            };
            text = comment[end + 2..].trim_start_matches(ascii_space);
        } else {
            break;
        }
    }
    [
        "select", "with", "insert", "update", "delete", "merge", "create", "alter", "drop",
        "truncate", "explain", "show", "desc", "describe", "call",
    ]
    .iter()
    .any(|word| {
        text.get(..word.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(word))
            && text
                .as_bytes()
                .get(word.len())
                .is_none_or(|next| !next.is_ascii_alphanumeric() && *next != b'_')
    })
}
pub fn system(
    dialect: Option<&str>,
    database: Option<&str>,
    schema: Option<&str>,
    structure: Option<&str>,
) -> String {
    let dialect = dialect.filter(|text| !blank(text)).unwrap_or("当前数据库");
    let database = database.filter(|text| !blank(text)).unwrap_or("未指定");
    let mut result = format!(
        "你是数据库 SQL 助手。只返回一条可直接放入编辑器的 SQL，不要 Markdown 代码块、解释或前后说明。\n必须使用给定数据库实际存在的表名和字段名，并遵循 {dialect} 方言。用户没有明确要求写操作时，只生成只读查询。不要执行 SQL。\n当前数据库：{database}"
    );
    if let Some(schema) = schema.filter(|text| !blank(text)) {
        result.push_str("，Schema：");
        result.push_str(schema);
    }
    if let Some(structure) = structure.filter(|text| !blank(text)) {
        result.push_str("\n\n当前库结构（只用于生成 SQL）：\n");
        result.push_str(structure);
    } else {
        result.push_str(
            "\n当前没有可用的表结构；无法确定名称时不要凭空编造，请在 SQL 注释中指出待替换名称。",
        );
    }
    result
}
pub fn user(request: Option<&str>, selected: Option<&str>) -> String {
    let mut result = format!("需求：{}", trim(request.unwrap_or_default()));
    if let Some(selected) = selected.filter(|text| !blank(text)) {
        result.push_str("\n\n需要参考或修改的现有 SQL：\n");
        result.push_str(trim(selected));
    }
    result
}
fn primitive(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        _ => anyhow::bail!("模型响应中没有文本内容"),
    }
}
pub fn content(body: &str) -> Result<String> {
    ensure!(body.len() <= MAX_TEXT, "模型响应超过 8 MiB");
    let mut root: Value = serde_json::from_str(body)?;
    let choices = root
        .get_mut("choices")
        .and_then(Value::as_array_mut)
        .filter(|choices| !choices.is_empty())
        .ok_or_else(|| anyhow::anyhow!("模型响应中没有 choices"))?;
    let message = choices[0]
        .get_mut("message")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("模型响应中没有文本内容"))?;
    let value = message
        .remove("content")
        .ok_or_else(|| anyhow::anyhow!("模型响应中没有文本内容"))?;
    match value {
        Value::String(_) | Value::Bool(_) | Value::Number(_) => primitive(value),
        Value::Array(parts) => {
            let mut result = String::new();
            for part in parts {
                if let Value::Object(mut part) = part
                    && let Some(value) = part.remove("text")
                {
                    let text = primitive(value)?;
                    ensure!(
                        result.len().saturating_add(text.len()) <= MAX_TEXT,
                        "模型响应超过 8 MiB"
                    );
                    result.push_str(&text);
                }
            }
            Ok(result)
        }
        _ => Ok(String::new()),
    }
}
fn shorten(text: &str) -> String {
    let text = trim(text);
    let mut result = String::new();
    let mut units = 0;
    for ch in text.chars() {
        if units + ch.len_utf16() > 500 {
            result.push('…');
            break;
        }
        units += ch.len_utf16();
        result.push(if matches!(ch, '\r' | '\n') { ' ' } else { ch });
    }
    result
}
pub fn failure(body: Option<&str>) -> String {
    let body = body.unwrap_or_default();
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(value) = value.get("error").and_then(|value| value.get("message"))
    {
        match value {
            Value::String(message) => return shorten(message),
            Value::Bool(_) | Value::Number(_) => return shorten(&value.to_string()),
            _ => {}
        }
    }
    shorten(if blank(body) { "未知错误" } else { body })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_ai_text_and_response_helpers_match_java_without_network_or_execution() -> Result<()>
    {
        let source: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/sql-ai/original-service.json"
        ))?;
        for case in source["urls"].as_array().unwrap() {
            assert_eq!(
                endpoint(case["input"].as_str()),
                case["output"].as_str().unwrap()
            );
        }
        for case in source["clean"].as_array().unwrap() {
            let value = clean(case["input"].as_str());
            assert_eq!(value, case["output"].as_str().unwrap(), "{}", case["input"]);
            assert_eq!(
                looks_sql(value),
                case["sql"].as_bool().unwrap(),
                "{}",
                case["input"]
            );
        }
        for case in source["system"].as_array().unwrap() {
            let values = &case["input"];
            assert_eq!(
                system(
                    values[0].as_str(),
                    values[1].as_str(),
                    values[2].as_str(),
                    values[3].as_str()
                ),
                case["output"].as_str().unwrap()
            );
        }
        for case in source["user"].as_array().unwrap() {
            let values = &case["input"];
            assert_eq!(
                user(values[0].as_str(), values[1].as_str()),
                case["output"].as_str().unwrap()
            );
        }
        for case in source["content"].as_array().unwrap() {
            let value = content(case["input"].as_str().unwrap());
            if let Some(error) = case["error"].as_str() {
                assert_eq!(value.unwrap_err().to_string(), error);
            } else {
                assert_eq!(value?, case["output"].as_str().unwrap());
            }
        }
        for case in source["errors"].as_array().unwrap() {
            assert_eq!(
                failure(case["input"].as_str()),
                case["output"].as_str().unwrap()
            );
        }
        Ok(())
    }
}
