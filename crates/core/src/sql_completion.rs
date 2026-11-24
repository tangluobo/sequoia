//! Native counterpart of SqlCompletionEngine; candidates borrow the catalog while filtering.
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, sync::LazyLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Keyword,
    Database,
    Table,
    Column,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Keyword => "关键字",
            Self::Database => "数据库",
            Self::Table => "表",
            Self::Column => "字段",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub text: String,
    pub kind: Kind,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default)]
    pub ai_tables: Vec<crate::ai_sql::editor::SchemaTable>,
    pub schema: String,
    pub database: String,
    pub databases: Vec<String>,
    pub namespaces: Vec<String>,
    pub tables: Vec<Table>,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub candidates: Vec<Candidate>,
}
pub const KEYWORDS: &str = "SELECT FROM WHERE INSERT INTO VALUES UPDATE SET DELETE CREATE DROP ALTER TABLE INDEX VIEW DATABASE AND OR NOT IN EXISTS BETWEEN LIKE IS NULL JOIN INNER LEFT RIGHT OUTER FULL CROSS ON GROUP BY ORDER HAVING LIMIT OFFSET UNION ALL AS DISTINCT CASE WHEN THEN ELSE END COUNT SUM AVG MIN MAX PRIMARY KEY FOREIGN REFERENCES CONSTRAINT DEFAULT CHECK UNIQUE AUTO_INCREMENT IF CASCADE RENAME TO BEGIN COMMIT ROLLBACK TRANSACTION GRANT REVOKE PRIVILEGES SHOW DESCRIBE EXPLAIN USE TRUNCATE CHARACTER COLLATE REPLACE";
fn normalize(identifier: &str) -> &str {
    let identifier = identifier.trim_matches(|ch: char| ch <= '\u{20}');
    for (open, close) in [('`', '`'), ('"', '"'), ('[', ']')] {
        if let Some(value) = identifier.strip_prefix(open) {
            return value.strip_suffix(close).unwrap_or(value);
        }
    }
    identifier
}
fn identifier(ch: char) -> bool {
    // Original Java scans UTF-16 char values; astral identifiers stop its scan.
    if ch as u32 > 0xffff {
        return false;
    }
    if ch.is_ascii() {
        return ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '.' | '`' | '"' | '[' | ']');
    }
    static LETTER_DIGIT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[\p{L}\p{Nd}]$").unwrap());
    LETTER_DIGIT.is_match(ch.encode_utf8(&mut [0; 4]))
}
fn folded(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    let first = upper.next().unwrap_or(ch);
    let upper = if upper.next().is_none() { first } else { ch };
    upper.to_lowercase().next().unwrap_or(upper)
}
fn compare_ci(left: &str, right: &str) -> Ordering {
    left.chars().map(folded).cmp(right.chars().map(folded))
}
fn blank(text: &str) -> bool {
    text.chars().all(|ch|matches!(ch,'\t'..='\r'|'\u{1c}'..='\u{20}'|'\u{1680}'|'\u{2000}'..='\u{2006}'|'\u{2008}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{205f}'|'\u{3000}'))
}
pub fn complete(text: &str, caret: usize, metadata: &Metadata) -> Completion {
    let mut empty = Completion {
        start: caret,
        ..Default::default()
    };
    if caret == 0 {
        return empty;
    }
    let mut chars = text.char_indices();
    let end = if let Some((index, _)) = chars.nth(caret) {
        index
    } else if text.chars().count() == caret {
        text.len()
    } else {
        return empty;
    };
    let start = text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| identifier(*ch))
        .last()
        .map_or(end, |(index, _)| index);
    let token = &text[start..end];
    if token.is_empty() {
        return empty;
    }
    let parts = token.split('.').collect::<Vec<_>>();
    let prefix = normalize(parts.last().copied().unwrap_or_default()).to_lowercase();
    let qualifier = &parts[..parts.len() - 1];
    if qualifier.iter().any(|part| normalize(part).is_empty()) {
        return empty;
    }
    empty.start = text[..start + token.rfind('.').map_or(0, |i| i + 1)]
        .chars()
        .count();
    let active = |name: &str| {
        let name = normalize(name);
        compare_ci(&metadata.database, name) == Ordering::Equal
            || metadata
                .namespaces
                .iter()
                .any(|item| compare_ci(item, name) == Ordering::Equal)
    };
    let table = |name: &str| {
        metadata
            .tables
            .iter()
            .find(|table| compare_ci(&table.name, normalize(name)) == Ordering::Equal)
    };
    enum Source<'a> {
        All,
        Tables,
        Columns(&'a Table),
    }
    let source = match qualifier {
        [] if prefix.is_empty() => return empty,
        [] => Source::All,
        [name] => {
            if let Some(table) = table(name) {
                Source::Columns(table)
            } else if active(name) {
                Source::Tables
            } else {
                return empty;
            }
        }
        _ => {
            if !qualifier[..qualifier.len() - 1]
                .iter()
                .any(|name| active(name))
            {
                return empty;
            }
            let Some(table) = table(qualifier[qualifier.len() - 1]) else {
                return empty;
            };
            Source::Columns(table)
        }
    };
    // Keep only the best 100 matches. Do not clone an entire catalog on every key.
    let mut matches = Vec::<(String, Candidate)>::new();
    let mut offer = |text: &str, kind: Kind| {
        if blank(text) {
            return;
        }
        let key = text.to_lowercase();
        if !key.starts_with(&prefix)
            || (!prefix.is_empty() && key == prefix)
            || matches.iter().any(|(old, _)| old == &key)
        {
            return;
        }
        let at = matches.partition_point(|(_, candidate)| {
            candidate
                .kind
                .cmp(&kind)
                .then_with(|| compare_ci(&candidate.text, text))
                != Ordering::Greater
        });
        if at >= 100 {
            return;
        }
        matches.insert(
            at,
            (
                key,
                Candidate {
                    text: text.into(),
                    kind,
                },
            ),
        );
        matches.truncate(100);
    };
    match source {
        Source::All => {
            for keyword in KEYWORDS.split_whitespace() {
                offer(keyword, Kind::Keyword);
            }
            for database in &metadata.databases {
                offer(database, Kind::Database);
            }
            for table in &metadata.tables {
                offer(&table.name, Kind::Table);
            }
            for table in &metadata.tables {
                for column in &table.columns {
                    offer(column, Kind::Column);
                }
            }
        }
        Source::Tables => {
            for table in &metadata.tables {
                offer(&table.name, Kind::Table);
            }
        }
        Source::Columns(table) => {
            for column in &table.columns {
                offer(column, Kind::Column);
            }
        }
    }
    empty.candidates = matches
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    empty
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qualified_names_choose_real_tables_and_columns_and_bound_the_popup() {
        let metadata = Metadata {
            schema: "public".into(),
            database: "test".into(),
            databases: vec!["test".into()],
            namespaces: vec!["public".into()],
            tables: vec![Table {
                name: "users".into(),
                columns: vec!["id".into(), "name".into()],
            }],
            ..Default::default()
        };
        for (sql, names) in [
            ("sel", vec!["SELECT"]),
            ("test.", vec!["users"]),
            ("users.", vec!["id", "name"]),
            ("public.users.n", vec!["name"]),
            ("other.users.", vec![]),
            ("users.name", vec![]),
        ] {
            assert_eq!(
                complete(sql, sql.chars().count(), &metadata)
                    .candidates
                    .iter()
                    .map(|candidate| candidate.text.as_str())
                    .collect::<Vec<_>>(),
                names,
                "{sql}"
            );
        }
        let metadata = Metadata {
            tables: (0..1000)
                .rev()
                .map(|i| Table {
                    name: format!("table_{i:04}"),
                    columns: vec![],
                })
                .collect(),
            ..Default::default()
        };
        let result = complete("table_", 6, &metadata);
        assert_eq!(result.candidates.len(), 100);
        assert_eq!(result.candidates[0].text, "table_0000");
        assert_eq!(result.candidates[99].text, "table_0099");
    }
}
