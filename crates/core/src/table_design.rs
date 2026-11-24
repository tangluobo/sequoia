//! TableStructureView's create-table draft and dialect-specific SQL projection.
use crate::workbench::TableId;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
pub mod alter;
pub mod auxiliary;
mod generation;
mod hive;
pub mod indexes;
mod inference;
pub use generation::Generation;
mod metadata;
pub mod mysql_definition;
mod options;
pub mod preview;
mod types;
pub use inference::{Suggestion, infer};
pub use metadata::{Charset, Metadata};
pub use types::data_types;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Dialect {
    #[serde(rename = "MYSQL")]
    Mysql,
    #[serde(rename = "POSTGRESQL")]
    Postgres,
    #[serde(rename = "ORACLE")]
    Oracle,
    #[serde(rename = "HIVE")]
    Hive,
}
impl Dialect {
    pub fn resolve(kind: &str) -> Result<Self> {
        Ok(match kind {
            "MYSQL" => Self::Mysql,
            "POSTGRESQL" => Self::Postgres,
            "ORACLE" => Self::Oracle,
            "HIVE" => Self::Hive,
            _ => bail!("当前数据库尚未实现表结构设计"),
        })
    }
    fn quote(self, name: &str) -> Result<String> {
        ensure!(!name.is_empty() && !name.contains('\0'), "对象名称无效");
        let quote = match self {
            Self::Mysql | Self::Hive => '`',
            _ => '"',
        };
        Ok(format!(
            "{quote}{}{quote}",
            name.replace(quote, &format!("{quote}{quote}"))
        ))
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Column {
    pub computed: Option<Computed>,
    pub generation: Generation,
    pub default_expression: bool,
    pub name: String,
    pub data_type: String,
    pub length: String,
    pub scale: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub default_value: String,
    pub comment: String,
    pub unsigned: bool,
    pub zerofill: bool,
    pub charset: String,
    pub collation: String,
    pub key_length: String,
    pub binary: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Computed {
    pub expression: String,
    pub stored: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub engine: String,
    pub charset: String,
    pub collation: String,
    pub auto_increment: String,
    pub row_format: String,
    pub avg_row_length: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Create {
    #[serde(default)]
    pub auxiliary: Vec<auxiliary::Edit>,
    #[serde(default)]
    pub indexes: Vec<indexes::Index>,
    pub table: TableId,
    pub columns: Vec<Column>,
    pub options: Options,
    pub comment: String,
}
fn blank(value: &str) -> bool {
    value.trim_matches(|ch: char| ch <= ' ').is_empty()
}
pub fn needs_decimal_places(data_type: &str) -> bool {
    let value = data_type.to_lowercase();
    ["decimal", "numeric", "double", "float", "real"]
        .iter()
        .any(|part| value.contains(part))
}
pub fn needs_length(data_type: &str) -> bool {
    let value = data_type.to_lowercase();
    [
        "varchar",
        "char",
        "decimal",
        "numeric",
        "varbinary",
        "binary",
        "bit",
    ]
    .iter()
    .any(|part| value.contains(part))
}
fn type_size(column: &Column) -> Result<String> {
    let kind = column.data_type.to_lowercase();
    let time = kind.contains("date")
        || kind.contains("time") && !kind.contains("interval")
        || kind.contains("year");
    if time {
        return if !column.scale.is_empty() && column.scale != "0" {
            number(&column.scale)?;
            Ok(format!("({})", column.scale))
        } else {
            Ok(String::new())
        };
    }
    if kind.contains("text")
        || kind.contains("blob")
        || [
            "json",
            "geometry",
            "point",
            "linestring",
            "polygon",
            "multipoint",
            "multilinestring",
            "multipolygon",
            "geometrycollection",
            "enum",
            "set",
            "bool",
            "boolean",
        ]
        .contains(&kind.as_str())
        || column.length.is_empty()
    {
        return Ok(String::new());
    }
    number(&column.length)?;
    if (needs_decimal_places(&column.data_type) || kind == "number") && !column.scale.is_empty() {
        number(column.scale.strip_prefix('-').unwrap_or(&column.scale))?;
        Ok(format!("({},{})", column.length, column.scale))
    } else {
        Ok(format!("({})", column.length))
    }
}
fn declaration(dialect: Dialect, column: &Column, backslash: bool) -> Result<String> {
    if matches!(dialect, Dialect::Hive) {
        hive::safe_type(&column.data_type)?;
    } else {
        safe_type(&column.data_type, backslash)?;
    }
    if matches!(dialect, Dialect::Postgres) && generation::postgres_integer(&column.data_type) {
        return Ok(column.data_type.clone());
    }
    if matches!(dialect, Dialect::Oracle) {
        let upper = column.data_type.to_ascii_uppercase();
        if upper == "TIMESTAMP"
            || upper.starts_with("TIMESTAMP(")
            || upper.starts_with("TIMESTAMP WITH ")
        {
            let suffix = upper.find(" WITH ").map_or("", |i| &column.data_type[i..]);
            let size = if column.scale.is_empty() {
                let base = &column.data_type[..column.data_type.len() - suffix.len()];
                let inline = &base[9..];
                if !inline.is_empty() {
                    let precision = inline
                        .strip_prefix('(')
                        .and_then(|v| v.strip_suffix(')'))
                        .ok_or_else(|| anyhow::anyhow!("Oracle 时间精度无效"))?;
                    number(precision)?;
                }
                inline.into()
            } else {
                number(&column.scale)?;
                format!("({})", column.scale)
            };
            ensure!(
                ["", " WITH TIME ZONE", " WITH LOCAL TIME ZONE"]
                    .contains(&suffix.to_ascii_uppercase().as_str()),
                "Oracle 时间类型无效"
            );
            return Ok(format!("{}{size}{suffix}", &column.data_type[..9]));
        }
        if upper == "NUMBER" && column.length.is_empty() && !column.scale.is_empty() {
            number(column.scale.strip_prefix('-').unwrap_or(&column.scale))?;
            return Ok(format!("NUMBER(*,{})", column.scale));
        }
        if matches!(upper.as_str(), "CHAR" | "VARCHAR2")
            && !column.length.is_empty()
            && matches!(column.charset.as_str(), "CHAR" | "BYTE")
        {
            number(&column.length)?;
            return Ok(format!(
                "{}({} {})",
                column.data_type, column.length, column.charset
            ));
        }
    }
    Ok(format!("{}{}", column.data_type, type_size(column)?))
}
fn number(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.bytes().all(|ch| ch.is_ascii_digit()),
        "字段长度和小数点必须是非负整数"
    );
    Ok(())
}
fn token(value: &str) -> Result<()> {
    ensure!(
        value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_'),
        "表选项名称无效"
    );
    Ok(())
}
fn literal(value: &str, backslash_escapes: bool) -> String {
    let value = if backslash_escapes {
        std::borrow::Cow::Owned(value.replace('\\', "\\\\"))
    } else {
        std::borrow::Cow::Borrowed(value)
    };
    format!("'{}'", value.replace('\'', "''"))
}

fn safe_type(value: &str, backslash: bool) -> Result<()> {
    ensure!(!blank(value), "字段类型无效");
    for token in mysql_definition::tokens(value, backslash)? {
        let text = &value[token.range];
        if text.starts_with(['(', '\'', '"', '`']) {
            continue;
        }
        ensure!(
            !text.contains([';', ','])
                && !["--", "/*", "*/"]
                    .iter()
                    .any(|marker| text.contains(marker)),
            "字段类型无效"
        );
    }
    Ok(())
}
impl Create {
    pub(super) fn validate_input(&self) -> Result<()> {
        ensure!(self.indexes.len() <= 1024, "索引超过 1,024 项");
        ensure!(
            self.indexes
                .iter()
                .map(|i| i.name.len()
                    + i.comment.len()
                    + i.signature.len()
                    + i.opaque.len()
                    + i.fields.iter().map(|f| f.name.len()).sum::<usize>())
                .sum::<usize>()
                <= 8 * 1024 * 1024,
            "索引定义超过 8 MiB"
        );
        ensure!(self.auxiliary.len() <= 2, "外键或触发器变更重复");
        ensure!(
            self.auxiliary
                .iter()
                .flat_map(|e| e.before.iter().chain(&e.after))
                .flat_map(|r| r.iter())
                .map(String::len)
                .sum::<usize>()
                <= 8 * 1024 * 1024,
            "外键或触发器定义超过 8 MiB"
        );
        ensure!(self.columns.len() <= 4096, "字段超过 4,096 项");
        let bytes = self.columns.iter().fold(
            self.table.schema.len()
                + self.table.name.len()
                + self.comment.len()
                + self.options.byte_len(),
            |total, column| {
                [
                    &column.name,
                    &column.data_type,
                    &column.length,
                    &column.scale,
                    &column.default_value,
                    &column.comment,
                    &column.charset,
                    &column.collation,
                    &column.key_length,
                ]
                .iter()
                .fold(total, |total, value| total.saturating_add(value.len()))
            },
        );
        let bytes = bytes
            + self
                .columns
                .iter()
                .filter_map(|column| column.computed.as_ref())
                .map(|c| c.expression.len())
                .sum::<usize>();
        ensure!(bytes <= 512 * 1024, "表定义超过 512 KiB");
        ensure!(
            self.columns.iter().all(|column| column
                .computed
                .as_ref()
                .is_none_or(|computed| !computed.expression.contains('\0'))),
            "生成表达式不能包含空字符"
        );
        ensure!(
            !self.table.schema.contains('\0')
                && !self.table.name.contains('\0')
                && !self.comment.contains('\0')
                && self.columns.iter().all(|c| [
                    &c.name,
                    &c.data_type,
                    &c.length,
                    &c.scale,
                    &c.default_value,
                    &c.comment,
                    &c.charset,
                    &c.collation,
                    &c.key_length
                ]
                .iter()
                .all(|s| !s.contains('\0'))),
            "表定义不能包含空字符"
        );
        Ok(())
    }

    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (index, column) in self.columns.iter().enumerate() {
            if blank(&column.name) {
                continue;
            }
            let prefix = format!("第{}行字段\"{}\"", index + 1, column.name);
            if blank(&column.data_type) {
                errors.push(format!("{prefix}：未设置类型"));
                continue;
            }
            if needs_length(&column.data_type) && blank(&column.length) {
                errors.push(format!(
                    "{prefix}：类型\"{}\"需要指定长度",
                    column.data_type
                ));
            }
            if needs_decimal_places(&column.data_type) && blank(&column.scale) {
                errors.push(format!(
                    "{prefix}：类型\"{}\"需要指定小数点",
                    column.data_type
                ));
            }
        }
        errors
    }
    pub fn sql(&self, dialect: Dialect) -> Result<String> {
        self.sql_for_session(dialect, false)
    }
    /// The actor supplies the actual MySQL/PostgreSQL string-literal mode.
    pub fn sql_for_session(&self, dialect: Dialect, backslash_escapes: bool) -> Result<String> {
        Ok(preview::render(
            dialect,
            &self.statements_for_session(dialect, backslash_escapes)?,
        ))
    }
    /// Execution uses separate generated statements, without reparsing user literals.
    pub fn statements_for_session(
        &self,
        dialect: Dialect,
        backslash_escapes: bool,
    ) -> Result<Vec<String>> {
        let backslash_escapes =
            matches!(dialect, Dialect::Mysql | Dialect::Postgres) && backslash_escapes;
        self.validate_input()?;
        ensure!(
            !matches!(dialect, Dialect::Hive)
                || self.columns.iter().all(|column| column.computed.is_none()),
            "Hive 不支持生成列"
        );
        let qualified = format!(
            "{}.{}",
            dialect.quote(&self.table.schema)?,
            dialect.quote(&self.table.name)?
        );
        let mut definitions = Vec::new();
        let mut primary = Vec::new();
        for column in &self.columns {
            if blank(&column.name) {
                continue;
            }
            let name = dialect.quote(&column.name)?;
            let mut sql = format!(
                "    {name} {}",
                declaration(dialect, column, backslash_escapes)?
            );
            match dialect {
                Dialect::Mysql if column.computed.is_some() => {
                    sql = format!(
                        "    {}",
                        alter::new_column(dialect, column, backslash_escapes)?
                    );
                }
                Dialect::Mysql => {
                    if column.unsigned {
                        sql.push_str(" UNSIGNED");
                    }
                    if column.zerofill {
                        sql.push_str(" ZEROFILL");
                    }
                    for (keyword, value) in [
                        (" CHARACTER SET ", &column.charset),
                        (" COLLATE ", &column.collation),
                    ] {
                        if !value.is_empty() {
                            token(value)?;
                            sql.push_str(keyword);
                            sql.push_str(value);
                        }
                    }
                    if column.not_null {
                        sql.push_str(" NOT NULL");
                    }
                    if column.auto_increment {
                        sql.push_str(" AUTO_INCREMENT");
                    }
                    if let Some(value) = column.default_sql(dialect, backslash_escapes)? {
                        sql.push_str(" DEFAULT ");
                        sql.push_str(&value);
                    }
                    if !column.comment.is_empty() {
                        sql.push_str(" COMMENT ");
                        sql.push_str(&literal(&column.comment, backslash_escapes));
                    }
                }
                Dialect::Postgres | Dialect::Oracle => {
                    sql = format!(
                        "    {}",
                        alter::new_column(dialect, column, backslash_escapes)?
                    );
                }
                Dialect::Hive => {
                    if !column.comment.is_empty() {
                        sql.push_str(" COMMENT ");
                        sql.push_str(&literal(&column.comment, false));
                    }
                }
            }
            definitions.push(sql);
            if column.primary_key && !matches!(dialect, Dialect::Hive) {
                primary.push(name);
            }
        }
        ensure!(!definitions.is_empty(), "请至少添加一个字段");
        if !primary.is_empty() {
            definitions.push(format!("    PRIMARY KEY ({})", primary.join(", ")));
        }
        let mut sql = format!("CREATE TABLE {qualified} (\n{}\n)", definitions.join(",\n"));
        if matches!(dialect, Dialect::Mysql) {
            for clause in self.options.mysql_clauses(None)? {
                sql.push(' ');
                sql.push_str(&clause);
            }
            if !self.comment.is_empty() {
                sql.push_str(" COMMENT=");
                sql.push_str(&literal(&self.comment, backslash_escapes));
            }
        } else if matches!(dialect, Dialect::Hive) && !self.comment.is_empty() {
            sql.push_str(" COMMENT ");
            sql.push_str(&literal(&self.comment, false));
        }
        if matches!(dialect, Dialect::Mysql) && !self.indexes.is_empty() {
            ensure!(self.indexes.len() <= 1024, "索引超过 1,024 项");
            // CREATE TABLE plus indexes is atomic for MySQL, including new-table drafts.
            let mut keys = Vec::new();
            let mut names = std::collections::BTreeSet::new();
            for index in &self.indexes {
                ensure!(
                    names.insert(indexes::identity(dialect, &index.name)),
                    "索引名称重复"
                );
                let statement =
                    index.create_sql(dialect, &self.table, &self.columns, backslash_escapes)?;
                let marker = format!(" ON {qualified}");
                let (prefix, suffix) = statement
                    .split_once(&marker)
                    .ok_or_else(|| anyhow::anyhow!("索引定义无效"))?;
                keys.push(format!(
                    "    {}{}",
                    prefix.strip_prefix("CREATE ").unwrap(),
                    suffix
                ));
            }
            let closing = sql
                .rfind("\n)")
                .ok_or_else(|| anyhow::anyhow!("建表定义无效"))?;
            sql.insert_str(closing, &(String::from(",\n") + &keys.join(",\n")));
        }
        sql.push(';');
        let mut statements = vec![sql];
        if matches!(dialect, Dialect::Postgres | Dialect::Oracle) && !self.comment.is_empty() {
            statements.push(format!(
                "COMMENT ON TABLE {qualified} IS {};",
                literal(&self.comment, backslash_escapes)
            ));
        }
        if matches!(dialect, Dialect::Postgres | Dialect::Oracle) {
            for column in self
                .columns
                .iter()
                .filter(|column| !blank(&column.name) && !column.comment.is_empty())
            {
                statements.push(format!(
                    "COMMENT ON COLUMN {qualified}.{} IS {};",
                    dialect.quote(&column.name)?,
                    literal(&column.comment, backslash_escapes)
                ));
            }
        }
        if !matches!(dialect, Dialect::Mysql) {
            ensure!(self.indexes.len() <= 1024, "索引超过 1,024 项");
            let mut names = std::collections::BTreeSet::new();
            for index in &self.indexes {
                ensure!(
                    names.insert(indexes::identity(dialect, &index.name)),
                    "索引名称重复"
                );
                statements.push(
                    index.create_sql(dialect, &self.table, &self.columns, backslash_escapes)? + ";",
                );
                if matches!(dialect, Dialect::Postgres) && !index.comment.is_empty() {
                    statements
                        .push(index.comment_sql(dialect, &self.table, backslash_escapes)? + ";");
                }
            }
        }
        for edit in &self.auxiliary {
            statements.extend(
                edit.creation(dialect, &self.table)?
                    .adds
                    .into_iter()
                    .map(|sql| {
                        if matches!(dialect, Dialect::Oracle) {
                            sql
                        } else {
                            sql + ";"
                        }
                    }),
            );
        }
        ensure!(
            statements.iter().map(String::len).sum::<usize>() + statements.len() - 1 <= 1024 * 1024,
            "建表语句超过 1 MiB"
        );
        ensure!(
            statements.iter().all(|sql| !sql.contains('\0')),
            "表定义不能包含空字符"
        );
        Ok(statements)
    }
}
#[cfg(test)]
mod tests;
