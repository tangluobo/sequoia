//! Borrowed, bounded formatting for the original result-table copy/export actions.
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
#[cfg(test)]
#[path = "result_transfer_tests.rs"]
mod tests;
pub const LINE_ENDING: &str = if cfg!(windows) { "\r\n" } else { "\n" };
pub struct Area<'a, F> {
    pub columns: &'a [String],
    pub rows: &'a [usize],
    pub selected_columns: &'a [usize],
    pub value: F,
}
#[derive(Clone, Copy)]
pub enum Format<'a> {
    Json {
        camel: bool,
    },
    Tsv {
        headers: bool,
        data: bool,
    },
    Comments,
    Insert {
        table: &'a str,
    },
    Update {
        table: &'a str,
        primary_keys: &'a [String],
    },
}
struct Output(String);
impl Output {
    fn push(&mut self, text: &str) -> Result<()> {
        ensure!(
            self.0.len().saturating_add(text.len()) <= 8 * 1024 * 1024,
            "复制或导出内容超过 8 MiB，请缩小范围"
        );
        self.0.push_str(text);
        Ok(())
    }
    fn ch(&mut self, ch: char) -> Result<()> {
        let mut bytes = [0; 4];
        self.push(ch.encode_utf8(&mut bytes))
    }
    fn escaped(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            match ch {
                '"' => self.push("\\\"")?,
                '\\' => self.push("\\\\")?,
                '\u{8}' => self.push("\\b")?,
                '\u{c}' => self.push("\\f")?,
                '\n' => self.push("\\n")?,
                '\r' => self.push("\\r")?,
                '\t' => self.push("\\t")?,
                ch if ch < ' ' => self.push(&format!("\\u{:04x}", u32::from(ch)))?,
                ch => self.ch(ch)?,
            }
        }
        Ok(())
    }
    fn single(&mut self, text: &str, tabs: bool) -> Result<()> {
        for ch in text.chars() {
            self.ch(if ch == '\r' || ch == '\n' || (tabs && ch == '\t') {
                ' '
            } else {
                ch
            })?;
        }
        Ok(())
    }
    fn literal(&mut self, value: Option<&str>) -> Result<()> {
        if let Some(value) = value {
            self.push("'")?;
            for part in value.split_inclusive('\'') {
                self.push(part)?;
                if part.ends_with('\'') {
                    self.push("'")?;
                }
            }
            self.push("'")
        } else {
            self.push("NULL")
        }
    }
}
fn whitespace(ch: char) -> bool {
    matches!(ch,'\t'..='\r'|'\u{1c}'..=' '|'\u{1680}'|'\u{2000}'..='\u{2006}'|'\u{2008}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{205f}'|'\u{3000}')
}
fn simple_upper(ch: char) -> char {
    let mut map = ch.to_uppercase();
    let first = map.next().unwrap_or(ch);
    if map.next().is_none() { first } else { ch }
}
pub fn camel(value: Option<&str>) -> String {
    let value = value.unwrap_or_default();
    let parts: Vec<_> = value
        .split(['_', '-', ' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
        .collect();
    if parts.len() == 1 {
        if value.to_uppercase() == value {
            return value.to_lowercase();
        }
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return String::new();
        };
        return first.to_lowercase().take(1).chain(chars).collect();
    }
    let mut result = String::new();
    for part in parts.into_iter().filter(|part| !part.is_empty()) {
        let lower = part.to_lowercase();
        if result.is_empty() {
            result.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                result.push(simple_upper(first));
                result.extend(chars);
            }
        }
    }
    if result.is_empty() {
        value.into()
    } else {
        result
    }
}
fn quoted(name: &str) -> String {
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_')
        && bytes.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$'));
    if valid {
        name.into()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}
pub fn format<'a, F>(
    area: Area<'a, F>,
    kind: Format<'_>,
    comments: &BTreeMap<String, String>,
) -> Result<String>
where
    F: Fn(usize, usize) -> Option<&'a str>,
{
    let mut out = Output(String::new());
    let columns = area.selected_columns;
    ensure!(
        columns.iter().all(|column| *column < area.columns.len()),
        "结果列索引已变化"
    );
    if matches!(kind, Format::Json { .. } | Format::Comments)
        && (area.rows.is_empty() || columns.is_empty())
    {
        return Ok(String::new());
    }
    match kind {
        Format::Json { camel: convert } => {
            out.push("[\n")?;
            for (order, row) in area.rows.iter().enumerate() {
                if order > 0 {
                    out.push(",\n")?;
                }
                out.push("  {\n")?;
                for (order, column) in columns.iter().enumerate() {
                    if order > 0 {
                        out.push(",\n")?;
                    }
                    out.push("    \"")?;
                    let field = &area.columns[*column];
                    if convert {
                        out.escaped(&camel(Some(field)))?;
                    } else {
                        out.escaped(field)?;
                    }
                    out.push("\": ")?;
                    if let Some(value) = (area.value)(*row, *column) {
                        out.push("\"")?;
                        out.escaped(value)?;
                        out.push("\"")?;
                    } else {
                        out.push("null")?;
                    }
                }
                out.push("\n  }")?;
            }
            out.push("\n]")?;
        }
        Format::Tsv { headers, data } => {
            if headers {
                for (index, column) in columns.iter().enumerate() {
                    if index > 0 {
                        out.push("\t")?;
                    }
                    out.single(&area.columns[*column], true)?;
                }
                out.push(LINE_ENDING)?;
            }
            if data {
                for row in area.rows {
                    for (index, column) in columns.iter().enumerate() {
                        if index > 0 {
                            out.push("\t")?;
                        }
                        out.single((area.value)(*row, *column).unwrap_or("NULL"), true)?;
                    }
                    out.push(LINE_ENDING)?;
                }
            }
            if !out.0.is_empty() {
                out.0.truncate(out.0.len() - LINE_ENDING.len());
            }
        }
        Format::Comments => {
            for (order, row) in area.rows.iter().enumerate() {
                if order > 0 {
                    out.push(LINE_ENDING)?;
                    out.push(LINE_ENDING)?;
                }
                for (order, column) in columns.iter().enumerate() {
                    if order > 0 {
                        out.push(LINE_ENDING)?;
                    }
                    let field = &area.columns[*column];
                    let comment = comments.get(field).or_else(|| {
                        comments
                            .iter()
                            .find(|(name, _)| crate::query_edit::identifier_eq(name, field))
                            .map(|(_, comment)| comment)
                    });
                    let label = comment
                        .filter(|comment| !comment.chars().all(whitespace))
                        .map_or(field.as_str(), String::as_str);
                    out.push(label)?;
                    out.push("：")?;
                    out.single((area.value)(*row, *column).unwrap_or("NULL"), false)?;
                }
            }
        }
        Format::Insert { table } | Format::Update { table, .. } => {
            if table.chars().all(whitespace) {
                return Ok(String::new());
            }
            let table = table.split('.').collect::<Vec<_>>();
            let end = table
                .iter()
                .rposition(|part| !part.is_empty())
                .map_or(0, |index| index + 1);
            let table = table
                .iter()
                .take(end)
                .map(|part| quoted(part))
                .collect::<Vec<_>>()
                .join(".");
            let mut emitted = 0usize;
            for row in area.rows {
                if let Format::Update { primary_keys, .. } = kind {
                    let assignments: Vec<_> = columns
                        .iter()
                        .map(|column| (*column, &area.columns[*column]))
                        .filter(|(_, name)| {
                            !primary_keys
                                .iter()
                                .any(|key| crate::query_edit::identifier_eq(key, name))
                        })
                        .collect();
                    let keys: Vec<_> = primary_keys
                        .iter()
                        .filter_map(|key| {
                            area.columns
                                .iter()
                                .position(|name| crate::query_edit::identifier_eq(name, key))
                                .map(|index| (key, index))
                        })
                        .collect();
                    if assignments.is_empty() || keys.len() != primary_keys.len() {
                        continue;
                    }
                    if emitted > 0 {
                        out.push(LINE_ENDING)?;
                    }
                    emitted += 1;
                    out.push("UPDATE ")?;
                    out.push(&table)?;
                    out.push(" SET ")?;
                    for (order, (column, name)) in assignments.iter().enumerate() {
                        if order > 0 {
                            out.push(", ")?;
                        }
                        out.push(&quoted(name))?;
                        out.push(" = ")?;
                        out.literal((area.value)(*row, *column))?;
                    }
                    out.push(" WHERE ")?;
                    for (order, (key, column)) in keys.iter().enumerate() {
                        if order > 0 {
                            out.push(" AND ")?;
                        }
                        out.push(&quoted(key))?;
                        if let Some(value) = (area.value)(*row, *column) {
                            out.push(" = ")?;
                            out.literal(Some(value))?;
                        } else {
                            out.push(" IS NULL")?;
                        }
                    }
                    out.push(";")?;
                } else {
                    if emitted > 0 {
                        out.push(LINE_ENDING)?;
                    }
                    emitted += 1;
                    out.push("INSERT INTO ")?;
                    out.push(&table)?;
                    out.push(" (")?;
                    for (position, column) in columns.iter().enumerate() {
                        let name = &area.columns[*column];
                        if position > 0 {
                            out.push(", ")?;
                        }
                        out.push(&quoted(name))?;
                    }
                    out.push(") VALUES (")?;
                    for (position, column) in columns.iter().enumerate() {
                        if position > 0 {
                            out.push(", ")?;
                        }
                        out.literal((area.value)(*row, *column))?;
                    }
                    out.push(");")?;
                }
            }
        }
    }
    Ok(out.0)
}
