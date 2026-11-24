//! Document policy uses Unicode scalar offsets, translated at the Java oracle boundary.
use super::{MAX_TEXT, trim};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::ops::Range;

fn byte(text: &str, caret: usize) -> usize {
    text.char_indices()
        .nth(caret)
        .map_or(text.len(), |(index, _)| index)
}
fn whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t'..='\r')
}
#[derive(Debug, PartialEq, Eq)]
pub struct Directive<'a> {
    pub prompt: &'a str,
    pub line_end: usize,
}
pub fn directive(text: &str, caret: usize) -> Option<Directive<'_>> {
    if text.is_empty() {
        return None;
    }
    let position = byte(text, caret);
    let start = text[..position].rfind('\n').map_or(0, |index| index + 1);
    let end = text[position..]
        .find('\n')
        .map_or(text.len(), |index| position + index);
    let line = text
        .get(start..end)?
        .strip_suffix('\r')
        .unwrap_or(&text[start..end]);
    let rest = line
        .trim_start_matches(whitespace)
        .strip_prefix("--")?
        .trim_start_matches(whitespace);
    if !rest.get(..2)?.eq_ignore_ascii_case("ai") {
        return None;
    }
    let rest = rest[2..].trim_start_matches(whitespace);
    let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))?;
    // Greedy \s* backtracks to leave one character for Java's .+?.
    if rest.is_empty() {
        return None;
    }
    let prompt = rest.trim_matches(whitespace);
    if prompt
        .chars()
        .any(|ch| matches!(ch, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}'))
    {
        return None;
    }
    Some(Directive {
        prompt: trim(prompt),
        line_end: text[..end].chars().count(),
    })
}
#[derive(Debug)]
pub struct Insertion {
    pub snapshot: String,
    pub position: usize,
    pub prefix: &'static str,
    pub suffix: &'static str,
}
impl Insertion {
    pub fn after(snapshot: &str, requested: usize) -> Self {
        let position = byte(snapshot, requested);
        let line_break = |ch| matches!(ch, '\r' | '\n');
        Self {
            snapshot: snapshot.into(),
            position,
            prefix: if snapshot[..position]
                .chars()
                .next_back()
                .is_some_and(|ch| !line_break(ch))
            {
                "\n"
            } else {
                ""
            },
            suffix: if snapshot[position..]
                .chars()
                .next()
                .is_some_and(|ch| !line_break(ch))
            {
                "\n"
            } else {
                ""
            },
        }
    }
    pub fn directive(snapshot: &str, value: &Directive<'_>) -> Self {
        let end = byte(snapshot, value.line_end);
        let mut insertion = Self::after(snapshot, value.line_end);
        if end < snapshot.len() {
            insertion.position = end + 1;
            insertion.prefix = "";
            insertion.suffix = "\n";
        } else {
            insertion.prefix = "\n";
            insertion.suffix = "";
        }
        insertion
    }
    /// Returns a replacement document and the generated SQL selection. Never executes SQL.
    pub fn apply(&self, current: &str, caret: usize, sql: &str) -> Result<(String, Range<usize>)> {
        let fallback;
        let insertion = if current == self.snapshot {
            self
        } else {
            fallback = Self::after(current, caret);
            &fallback
        };
        let size = current
            .len()
            .checked_add(sql.len())
            .and_then(|size| size.checked_add(insertion.prefix.len() + insertion.suffix.len()));
        ensure!(
            size.is_some_and(|size| size <= MAX_TEXT),
            "SQL 文档超过 8 MiB，生成结果未插入"
        );
        let mut result = String::with_capacity(size.unwrap());
        result.push_str(&current[..insertion.position]);
        result.push_str(insertion.prefix);
        let start = result.chars().count();
        result.push_str(sql);
        let end = start + sql.chars().count();
        result.push_str(insertion.suffix);
        result.push_str(&current[insertion.position..]);
        Ok((result, start..end))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SchemaTable {
    pub text: String,
    pub terms: Vec<String>,
    pub order: usize,
}
fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| !whitespace(*ch) && !matches!(ch, '_' | '-' | '`' | '"' | '.'))
        .collect()
}
pub fn schema_context(tables: &[SchemaTable], prompt: &str) -> String {
    let prompt = normalize(prompt);
    let mut ranked: Vec<_> = tables
        .iter()
        .map(|table| {
            let score: usize = table
                .terms
                .iter()
                .map(|term| {
                    let term = normalize(term);
                    let size = term.encode_utf16().count();
                    if size >= 2 && prompt.contains(&term) {
                        size.min(20)
                    } else {
                        0
                    }
                })
                .sum();
            (table, score)
        })
        .collect();
    ranked.sort_by(|(left, a), (right, b)| b.cmp(a).then_with(|| left.order.cmp(&right.order)));
    let mut result = String::new();
    let mut units = 0;
    let mut included = 0;
    for (table, _) in &ranked {
        let extra = table.text.encode_utf16().count() + if result.is_empty() { 0 } else { 2 };
        if included >= 80 || units + extra > 48_000 {
            break;
        }
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.push_str(&table.text);
        units += extra;
        included += 1;
    }
    if included < tables.len() {
        result.push_str("\n\n-- 其余表结构因上下文长度限制未发送");
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_editor_directives_insertions_and_schema_ranking() -> Result<()> {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/sql-ai/original-editor.json"
        ))?;
        for case in source["directives"].as_array().unwrap() {
            if case.get("error").is_some() {
                continue;
            }
            let actual = directive(
                case["text"].as_str().unwrap(),
                case["caret"].as_u64().unwrap() as usize,
            );
            if case["output"].is_null() {
                assert!(actual.is_none(), "{case}");
            } else {
                let actual = actual.unwrap_or_else(|| panic!("{case}"));
                assert_eq!(
                    actual.prompt,
                    case["output"]["prompt"].as_str().unwrap(),
                    "{case}"
                );
                assert_eq!(
                    actual.line_end,
                    case["lineEndCodepoints"].as_u64().unwrap() as usize,
                    "{case}"
                );
            }
        }
        for case in source["insertions"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let actual = Insertion::after(text, case["caret"].as_u64().unwrap() as usize);
            assert_eq!(
                text[..actual.position].encode_utf16().count(),
                case["output"]["start"].as_u64().unwrap() as usize
            );
            assert_eq!(actual.prefix, case["output"]["prefix"].as_str().unwrap());
            assert_eq!(actual.suffix, case["output"]["suffix"].as_str().unwrap());
        }
        for case in source["contexts"].as_array().unwrap() {
            let tables: Vec<SchemaTable> = serde_json::from_value(case["tables"].clone())?;
            assert_eq!(
                schema_context(&tables, case["prompt"].as_str().unwrap()),
                case["output"].as_str().unwrap()
            );
        }
        for case in source["edits"].as_array().unwrap() {
            let snapshot = case["snapshot"].as_str().unwrap();
            let insertion = Insertion::after(snapshot, snapshot.chars().count());
            let (text, selection) = insertion.apply(
                case["current"].as_str().unwrap(),
                case["caret"].as_u64().unwrap() as usize,
                case["sql"].as_str().unwrap(),
            )?;
            assert_eq!(text, case["output"].as_str().unwrap());
            assert_eq!(
                selection.start,
                case["selectionStart"].as_u64().unwrap() as usize
            );
            assert_eq!(
                selection.end,
                case["selectionEnd"].as_u64().unwrap() as usize
            );
        }
        Ok(())
    }
}
