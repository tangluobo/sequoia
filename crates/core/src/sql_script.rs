//! Legacy SqlSplitter semantics, with a borrowed streaming statement iterator.
use anyhow::{Result, bail};
use regex::Regex;
use std::sync::LazyLock;
mod oracle;
mod procedural;

pub(crate) fn is_oracle_procedural(sql: &str) -> bool {
    oracle::procedural(sql)
}

/// Oracle procedural statements retain their internal semicolons. A SQL*Plus
/// slash on a line of its own separates successive procedural units.
pub fn split_for<'a>(kind: &str, sql: Option<&'a str>) -> DialectStatements<'a> {
    split_for_session(kind, sql, kind == "MYSQL")
}
pub fn split_for_session<'a>(
    kind: &str,
    sql: Option<&'a str>,
    backslash: bool,
) -> DialectStatements<'a> {
    DialectStatements {
        regular: split(sql),
        oracle: oracle::Statements::new(sql.unwrap_or_default()),
        procedural: procedural::Statements::with_escape(
            sql.unwrap_or_default(),
            kind == "MYSQL",
            backslash,
        ),
        kind: match kind {
            "ORACLE" => LexicalDialect::Oracle,
            "MYSQL" | "POSTGRESQL" => LexicalDialect::Procedural,
            _ => LexicalDialect::Regular,
        },
    }
}
#[derive(Clone, Copy)]
enum LexicalDialect {
    Regular,
    Oracle,
    Procedural,
}
pub struct DialectStatements<'a> {
    regular: Statements<'a>,
    oracle: oracle::Statements<'a>,
    procedural: procedural::Statements<'a>,
    kind: LexicalDialect,
}
impl DialectStatements<'_> {
    pub fn set_backslash(&mut self, value: bool) {
        self.procedural.set_backslash(value);
    }
}
impl<'a> Iterator for DialectStatements<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        match self.kind {
            LexicalDialect::Oracle => self.oracle.next(),
            LexicalDialect::Procedural => self.procedural.next(),
            LexicalDialect::Regular => self.regular.next(),
        }
    }
}

pub struct Statements<'a> {
    remaining: &'a str,
}
#[derive(Clone, Copy)]
enum State {
    Normal,
    Single,
    Double,
    Line,
    Block,
}
pub fn split(sql: Option<&str>) -> Statements<'_> {
    Statements {
        remaining: sql.unwrap_or_default(),
    }
}
fn trim(text: &str) -> &str {
    text.trim_matches(|ch: char| ch <= ' ')
}
impl<'a> Iterator for Statements<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining.is_empty() {
                return None;
            }
            let text = self.remaining;
            let bytes = text.as_bytes();
            let mut state = State::Normal;
            let mut index = 0;
            let mut end = bytes.len();
            while index < bytes.len() {
                let ch = bytes[index];
                let next = bytes.get(index + 1).copied().unwrap_or(0);
                match state {
                    State::Normal => match (ch, next) {
                        (b'\'', _) => state = State::Single,
                        (b'"', _) => state = State::Double,
                        (b'-', b'-') => {
                            state = State::Line;
                            index += 1;
                        }
                        (b'/', b'*') => {
                            state = State::Block;
                            index += 1;
                        }
                        (b';', _) => {
                            end = index;
                            break;
                        }
                        _ => {}
                    },
                    State::Single if ch == b'\'' => {
                        if next == b'\'' {
                            index += 1;
                        } else {
                            state = State::Normal;
                        }
                    }
                    State::Double if ch == b'"' => {
                        if next == b'"' {
                            index += 1;
                        } else {
                            state = State::Normal;
                        }
                    }
                    State::Line if ch == b'\n' => state = State::Normal,
                    State::Block if ch == b'*' && next == b'/' => {
                        index += 1;
                        state = State::Normal;
                    }
                    _ => {}
                }
                index += 1;
            }
            self.remaining = if end < bytes.len() {
                &text[end + 1..]
            } else {
                ""
            };
            let statement = trim(&text[..end]);
            if !statement.is_empty() {
                return Some(statement);
            }
        }
    }
}
use crate::java_text::is_whitespace as java_whitespace;
fn strip_leading(mut text: &str) -> &str {
    loop {
        text = text.trim_start_matches(java_whitespace);
        if let Some(rest) = text.strip_prefix("--") {
            text = rest.find('\n').map_or("", |index| &rest[index + 1..]);
        } else if let Some(rest) = text.strip_prefix("/*") {
            text = rest.find("*/").map_or("", |index| &rest[index + 2..]);
        } else {
            return text;
        }
    }
}
pub fn is_select(sql: Option<&str>) -> bool {
    let text = trim(strip_leading(sql.unwrap_or_default())).to_uppercase();
    ["SELECT", "WITH", "SHOW", "DESCRIBE", "DESC", "EXPLAIN"]
        .iter()
        .any(|keyword| text.starts_with(keyword))
}
fn letter(ch: char, digit: bool) -> bool {
    if ch.is_ascii() {
        return ch.is_ascii_alphabetic() || (digit && ch.is_ascii_digit());
    }
    // Java's char-based helpers see supplementary characters as two nonletters.
    if ch as u32 > 0xffff {
        return false;
    }
    static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\p{L}\z").unwrap());
    static ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}\p{Nd}]\z").unwrap());
    let mut bytes = [0; 4];
    let value = ch.encode_utf8(&mut bytes);
    if digit {
        ALNUM.is_match(value)
    } else {
        LETTER.is_match(value)
    }
}
fn word_edge(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| ch != '_' && !letter(ch, true))
}
fn simple_upper(ch: char, expected: u8) -> bool {
    if ch == 'İ' && expected == b'I' {
        return true;
    }
    let mut upper = ch.to_uppercase();
    upper.next() == Some(char::from(expected)) && upper.next().is_none()
}
fn keyword(text: &str, index: usize, word: &str) -> bool {
    let mut chars = text[index..].char_indices();
    let mut end = index;
    for expected in word.bytes() {
        let Some((offset, ch)) = chars.next() else {
            return false;
        };
        if !simple_upper(ch, expected) {
            return false;
        }
        end = index + offset + ch.len_utf8();
    }
    word_edge(text[..index].chars().next_back()) && word_edge(text[end..].chars().next())
}
fn top_level(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut depth = 0usize;
    let mut quote = None;
    text.char_indices().filter(move |&(_, ch)| {
        if let Some(open) = quote {
            if (open == '[' && ch == ']') || (open != '[' && ch == open) {
                quote = None;
            }
            return false;
        }
        if matches!(ch, '\'' | '"' | '`' | '[') {
            quote = Some(ch);
            return false;
        }
        if ch == '(' {
            depth += 1;
            return false;
        }
        if ch == ')' {
            depth = depth.saturating_sub(1);
            return false;
        }
        depth == 0
    })
}
fn multiple_sources(text: &str) -> bool {
    if top_level(text).any(|(index, _)| {
        ["UNION", "EXCEPT", "INTERSECT"]
            .iter()
            .any(|word| keyword(text, index, word))
    }) {
        return true;
    }
    for (index, ch) in top_level(text) {
        if ch == ',' || keyword(text, index, "JOIN") {
            return true;
        }
        if [
            "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "FOR",
        ]
        .iter()
        .any(|word| keyword(text, index, word))
        {
            return false;
        }
    }
    false
}
fn find_from(text: &str) -> Option<usize> {
    let upper = text.to_uppercase();
    let mut depth = 0i64;
    let mut single = false;
    let mut double = false;
    let mut units = 0;
    for (index, ch) in upper.char_indices() {
        let offset = units;
        units += ch.len_utf16();
        if ch == '\'' && !double {
            single = !single;
            continue;
        }
        if ch == '"' && !single {
            double = !double;
            continue;
        }
        if single || double {
            continue;
        }
        if ch == '(' {
            depth += 1;
            continue;
        }
        if ch == ')' {
            depth -= 1;
            continue;
        }
        if depth == 0
            && upper[index..].starts_with("FROM")
            && word_edge(upper[..index].chars().next_back())
            && word_edge(upper[index + 4..].chars().next())
        {
            return Some(offset);
        }
    }
    None
}
fn utf16_offset(text: &str, target: usize) -> Result<usize> {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units == target {
            return Ok(index);
        }
        units += ch.len_utf16();
        if units > target {
            bail!("源表位置落在 UTF-16 代理项内部");
        }
    }
    if units == target {
        Ok(text.len())
    } else {
        bail!("原版源表位置超出 SQL 长度")
    }
}
fn upper_prefix(text: &str, word: &str) -> bool {
    let mut chars = text.chars().flat_map(char::to_uppercase);
    word.chars().all(|ch| chars.next() == Some(ch))
}
pub fn source_table(sql: Option<&str>) -> Result<Option<String>> {
    let text = trim(strip_leading(sql.unwrap_or_default()));
    if !upper_prefix(text, "SELECT") {
        return Ok(None);
    }
    let Some(from) = find_from(text) else {
        return Ok(None);
    };
    let after = trim(&text[utf16_offset(text, from + 4)?..]);
    if multiple_sources(after) {
        return Ok(None);
    }
    let mut result = String::new();
    let mut quote = None;
    for (index, ch) in after.char_indices() {
        if let Some(open) = quote {
            if (open == '[' && ch == ']') || (open != '[' && ch == open) {
                quote = None;
            } else {
                result.push(ch);
            }
            continue;
        }
        if matches!(ch, '`' | '"' | '[') {
            quote = Some(ch);
            continue;
        }
        if java_whitespace(ch) || matches!(ch, ',' | ';' | '(' | ')') {
            break;
        }
        if index > 0
            && letter(ch, false)
            && [
                "WHERE",
                "JOIN",
                "INNER",
                "LEFT",
                "RIGHT",
                "LIMIT",
                "ORDER",
                "GROUP",
                "HAVING",
                "UNION",
                "EXCEPT",
                "INTERSECT",
                "FOR",
                "AS",
            ]
            .iter()
            .any(|word| upper_prefix(&after[index..], word))
        {
            break;
        }
        result.push(ch);
    }
    let result = trim(&result);
    Ok((!result.is_empty()).then(|| result.to_owned()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_classification_and_safe_table_targets_match_actual_java() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/sql-script/original.json"))
                .unwrap();
        for case in golden.as_array().unwrap() {
            let input = case["input"].as_str();
            let statements: Vec<_> = split(input).collect();
            assert_eq!(
                serde_json::to_value(statements).unwrap(),
                case["statements"],
                "{input:?}"
            );
            assert_eq!(
                is_select(input),
                case["select"].as_bool().unwrap(),
                "{input:?}"
            );
            let table = source_table(input);
            if case.get("tableError").is_some() {
                assert!(table.is_err(), "{input:?}");
            } else {
                assert_eq!(
                    serde_json::to_value(table.unwrap()).unwrap(),
                    case["table"],
                    "{input:?}"
                );
            }
        }
    }
    #[test]
    fn iterator_borrows_each_statement_without_retaining_a_second_script_or_statement_list() {
        let text = "SELECT '中文;'; SELECT 2;";
        let first = split(Some(text)).next().unwrap();
        assert_eq!(first.as_ptr(), text.as_ptr());
        assert!(std::mem::size_of::<Statements<'_>>() <= 2 * std::mem::size_of::<usize>());
    }
}
