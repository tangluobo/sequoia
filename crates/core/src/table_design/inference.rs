//! Original field-name defaults, applied only to an unset type and empty size cells.
use super::{Column, Dialect};
use regex::Regex;
use std::sync::LazyLock;
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Suggestion {
    #[serde(rename = "type")]
    pub data_type: &'static str,
    pub length: &'static str,
    #[serde(rename = "decimalPlaces")]
    pub scale: &'static str,
}
#[derive(Clone, Copy)]
enum Kind {
    Id,
    Date,
    Time,
    Datetime,
    Uuid,
    Boolean,
    Integer,
    Decimal,
    LongText,
    Binary,
    Json,
    String,
}
fn classify(field: &str) -> Kind {
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new("([a-z0-9])([A-Z])").unwrap());
    static ACRONYM: LazyLock<Regex> = LazyLock::new(|| Regex::new("([A-Z]+)([A-Z][a-z])").unwrap());
    static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9\p{Han}]+").unwrap());
    let original = field.trim_matches(|ch: char| ch <= ' ');
    let camel = CAMEL.replace_all(original, "${1}_${2}");
    let acronym = ACRONYM.replace_all(&camel, "${1}_${2}");
    let lower = acronym.to_lowercase();
    let normalized = SEPARATOR.replace_all(&lower, "_");
    let tokens: Vec<_> = normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect();
    let has = |values: &[&str]| tokens.iter().any(|token| values.contains(token));
    let contains = |values: &[&str]| values.iter().any(|value| original.contains(value));
    if has(&["uuid", "guid"]) || contains(&["唯一标识"]) {
        return Kind::Uuid;
    }
    if has(&["id"]) || contains(&["编号", "主键"]) {
        return Kind::Id;
    }
    let date = has(&["date", "day", "birthday", "birthdate"]) || contains(&["日期", "生日"]);
    let time = has(&["time"]) || contains(&["时间"]);
    if has(&["datetime", "datatime", "timestamp"])
        || tokens.len() > 1 && tokens.last() == Some(&"at")
        || contains(&[
            "日期时间",
            "时间戳",
            "创建时间",
            "更新时间",
            "修改时间",
            "删除时间",
        ])
        || date && time
    {
        return Kind::Datetime;
    }
    if date {
        return Kind::Date;
    }
    if time {
        return Kind::Time;
    }
    if has(&["json", "jsonb"]) {
        return Kind::Json;
    }
    if has(&["binary", "blob", "bytes"]) {
        return Kind::Binary;
    }
    if has(&[
        "body",
        "content",
        "description",
        "detail",
        "details",
        "memo",
        "remark",
        "remarks",
        "text",
    ]) {
        return Kind::LongText;
    }
    if tokens
        .first()
        .is_some_and(|token| ["is", "has", "can"].contains(token))
        || has(&[
            "active",
            "available",
            "deleted",
            "disabled",
            "enabled",
            "flag",
            "locked",
            "required",
            "valid",
            "visible",
        ])
        || contains(&["是否"])
    {
        return Kind::Boolean;
    }
    if has(&[
        "amount",
        "balance",
        "cost",
        "discount",
        "height",
        "percent",
        "percentage",
        "price",
        "rate",
        "ratio",
        "salary",
        "score",
        "weight",
    ]) {
        return Kind::Decimal;
    }
    if has(&[
        "age", "count", "index", "level", "number", "num", "order", "position", "quantity", "qty",
        "rank", "sequence", "seq", "sort", "version",
    ]) {
        return Kind::Integer;
    }
    Kind::String
}
pub fn infer(dialect: Dialect, field: &str, available: &[&str]) -> Option<Suggestion> {
    if crate::java_text::is_blank(field) {
        return None;
    }
    let json = available
        .iter()
        .any(|value| value.eq_ignore_ascii_case("JSON"));
    let kind = classify(field);
    let (data_type, length, scale) = match dialect {
        Dialect::Mysql => match kind {
            Kind::Id => ("BIGINT", "", ""),
            Kind::Date => ("DATE", "", ""),
            Kind::Time => ("TIME", "", ""),
            Kind::Datetime => ("DATETIME", "", ""),
            Kind::Uuid => ("CHAR", "36", ""),
            Kind::Boolean => ("BOOLEAN", "", ""),
            Kind::Integer => ("INT", "", ""),
            Kind::Decimal => ("DECIMAL", "18", "2"),
            Kind::LongText => ("TEXT", "", ""),
            Kind::Binary => ("BLOB", "", ""),
            Kind::Json if json => ("JSON", "", ""),
            Kind::Json => ("LONGTEXT", "", ""),
            Kind::String => ("VARCHAR", "255", ""),
        },
        Dialect::Postgres => match kind {
            Kind::Id => ("bigint", "", ""),
            Kind::Date => ("date", "", ""),
            Kind::Time => ("time", "", ""),
            Kind::Datetime => ("timestamp", "", ""),
            Kind::Uuid => ("uuid", "", ""),
            Kind::Boolean => ("boolean", "", ""),
            Kind::Integer => ("integer", "", ""),
            Kind::Decimal => ("numeric", "18", "2"),
            Kind::LongText => ("text", "", ""),
            Kind::Binary => ("bytea", "", ""),
            Kind::Json => ("jsonb", "", ""),
            Kind::String => ("character varying", "255", ""),
        },
        Dialect::Oracle => match kind {
            Kind::Id => ("NUMBER", "19", ""),
            Kind::Date => ("DATE", "", ""),
            Kind::Time | Kind::Datetime => ("TIMESTAMP", "", ""),
            Kind::Uuid => ("VARCHAR2", "36", ""),
            Kind::Boolean => ("NUMBER", "1", ""),
            Kind::Integer => ("NUMBER", "10", ""),
            Kind::Decimal => ("NUMBER", "", ""),
            Kind::LongText => ("CLOB", "", ""),
            Kind::Binary => ("BLOB", "", ""),
            Kind::Json if json => ("JSON", "", ""),
            Kind::Json => ("CLOB", "", ""),
            Kind::String => ("VARCHAR2", "255", ""),
        },
        Dialect::Hive => match kind {
            Kind::Id => ("BIGINT", "", ""),
            Kind::Date => ("DATE", "", ""),
            Kind::Time | Kind::Datetime => ("TIMESTAMP", "", ""),
            Kind::Uuid | Kind::String | Kind::LongText | Kind::Json => ("STRING", "", ""),
            Kind::Boolean => ("BOOLEAN", "", ""),
            Kind::Integer => ("INT", "", ""),
            Kind::Decimal => ("DECIMAL", "18", "2"),
            Kind::Binary => ("BINARY", "", ""),
        },
    };
    Some(Suggestion {
        data_type,
        length,
        scale,
    })
}
impl Column {
    pub fn infer_if_unset(&mut self, dialect: Dialect, available: &[&str]) -> bool {
        if !crate::java_text::is_blank(&self.data_type) {
            return false;
        }
        let Some(suggestion) = infer(dialect, &self.name, available) else {
            return false;
        };
        self.data_type = suggestion.data_type.into();
        if crate::java_text::is_blank(&self.length) && !suggestion.length.is_empty() {
            self.length = suggestion.length.into();
        }
        if crate::java_text::is_blank(&self.scale) && !suggestion.scale.is_empty() {
            self.scale = suggestion.scale.into();
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_acronyms_chinese_priorities_json_support_and_whitespace_match_original() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/table-design/original-inference.json"
        ))
        .unwrap();
        assert!(rows.as_array().unwrap().len() > 400);
        for row in rows.as_array().unwrap() {
            let kind: Dialect = serde_json::from_value(row["kind"].clone()).unwrap();
            let available: Vec<_> = row["available"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            let actual = infer(kind, row["field"].as_str().unwrap_or_default(), &available);
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                row["expected"],
                "{} {}",
                row["kind"],
                row["field"]
            );
        }
    }
    #[test]
    fn defaults_never_replace_an_existing_type_or_user_entered_size() {
        let mut column = Column {
            name: "amount".into(),
            length: "30".into(),
            scale: "5".into(),
            ..Default::default()
        };
        assert!(column.infer_if_unset(Dialect::Mysql, &[]));
        assert_eq!(
            (&*column.data_type, &*column.length, &*column.scale),
            ("DECIMAL", "30", "5")
        );
        column.name = "description".into();
        assert!(!column.infer_if_unset(Dialect::Mysql, &[]));
        assert_eq!(column.data_type, "DECIMAL");
        column.data_type = "\u{2003}".into();
        column.length.clear();
        column.scale.clear();
        assert!(column.infer_if_unset(Dialect::Mysql, &[]));
        assert_eq!(column.data_type, "TEXT");
        column.name = "title".into();
        column.data_type = "\u{00a0}".into();
        assert!(!column.infer_if_unset(Dialect::Mysql, &[]));
    }
}
