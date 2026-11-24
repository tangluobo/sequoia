//! Editable server snapshot and lossless, bounded ALTER projection.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InfoPage {
    Indexes,
    ForeignKeys,
    Triggers,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InfoTable {
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub index_definitions: Vec<indexes::Index>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub definition: Create,
    #[serde(default)]
    pub extra_options: [String; 3],
    #[serde(default)]
    pub backslash_escapes: bool,
    pub raw_columns: Vec<String>,
    pub indexes: InfoTable,
    pub foreign_keys: InfoTable,
    pub triggers: InfoTable,
    pub sql: String,
    pub view: bool,
    pub primary_name: Option<String>,
}
/// Only columns and constraint identity are needed while a save is queued.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Baseline {
    pub definition: Create,
    pub raw_columns: Vec<String>,
    pub primary_name: Option<String>,
    pub view: bool,
}
impl Snapshot {
    pub fn baseline(&self) -> Baseline {
        Baseline {
            definition: self.definition.clone(),
            raw_columns: self.raw_columns.clone(),
            primary_name: self.primary_name.clone(),
            view: self.view,
        }
    }
}
impl From<Snapshot> for Baseline {
    fn from(snapshot: Snapshot) -> Self {
        Self {
            definition: snapshot.definition,
            raw_columns: snapshot.raw_columns,
            primary_name: snapshot.primary_name,
            view: snapshot.view,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub before: Baseline,
    pub after: Create,
    pub origins: Vec<Option<String>>,
}
#[derive(Debug)]
pub struct Plan {
    pub statements: Vec<String>,
    pub reorder_unsupported: bool,
}
fn qualified(dialect: Dialect, table: &TableId) -> Result<String> {
    Ok(format!(
        "{}.{}",
        dialect.quote(&table.schema)?,
        dialect.quote(&table.name)?
    ))
}
fn default(value: &str, backslash: bool) -> String {
    if value.eq_ignore_ascii_case("NULL") || value.eq_ignore_ascii_case("CURRENT_TIMESTAMP") {
        value.to_ascii_uppercase()
    } else {
        literal(value, backslash)
    }
}
fn column_type(dialect: Dialect, column: &Column, backslash: bool) -> Result<String> {
    declaration(dialect, column, backslash)
}
pub(super) fn new_column(dialect: Dialect, column: &Column, backslash: bool) -> Result<String> {
    let mut value = format!(
        "{} {}",
        dialect.quote(&column.name)?,
        column_type(dialect, column, backslash)?
    );
    if matches!(dialect, Dialect::Oracle | Dialect::Postgres) {
        if let Some(clause) = column.computed_clause(dialect, backslash)? {
            if matches!(dialect, Dialect::Oracle) {
                value = dialect.quote(&column.name)?;
            }
            value.push_str(&clause);
        } else if let Some(clause) = column.identity_clause(dialect)? {
            value.push_str(clause);
        }
        if let Some(default) = column.default_sql(dialect, backslash)? {
            value.push_str(&format!(" DEFAULT {default}"));
        }
        if column.not_null {
            value.push_str(" NOT NULL");
        }
        return Ok(value);
    }
    if matches!(dialect, Dialect::Mysql) {
        if column.unsigned {
            value.push_str(" UNSIGNED");
        }
        if column.zerofill {
            value.push_str(" ZEROFILL");
        }
        for (prefix, s) in [
            (" CHARACTER SET ", &column.charset),
            (" COLLATE ", &column.collation),
        ] {
            if !s.is_empty() {
                token(s)?;
                value.push_str(prefix);
                value.push_str(s);
            }
        }
    }
    if let Some(clause) = column.computed_clause(dialect, backslash)? {
        value.push_str(&clause);
    }
    if column.not_null {
        value.push_str(" NOT NULL");
    }
    if matches!(dialect, Dialect::Mysql) && column.auto_increment {
        value.push_str(" AUTO_INCREMENT");
    }
    if let Some(default) = column.default_sql(dialect, backslash)? {
        value.push_str(" DEFAULT ");
        value.push_str(&default);
    }
    if matches!(dialect, Dialect::Mysql) && !column.comment.is_empty() {
        value.push_str(" COMMENT ");
        value.push_str(&literal(&column.comment, backslash));
    }
    Ok(value)
}
impl Edit {
    pub fn plan(&self, dialect: Dialect, backslash: bool) -> Result<Plan> {
        if matches!(dialect, Dialect::Hive) {
            ensure!(
                self.after.auxiliary.is_empty(),
                "Hive 不支持外键或触发器设计"
            );
            return super::hive::plan(self);
        }
        ensure!(
            matches!(
                dialect,
                Dialect::Mysql | Dialect::Postgres | Dialect::Oracle
            ),
            "当前数据库尚未实现保存表结构"
        );
        ensure!(!self.before.view, "视图字段不能通过 ALTER TABLE 修改");
        ensure!(
            self.after.table == self.before.definition.table,
            "表标识不一致"
        );
        ensure!(
            self.after.columns.len() <= 4096 && self.origins.len() == self.after.columns.len(),
            "字段标识不完整"
        );
        self.after.validate_input()?;
        let old = &self.before.definition;
        let table = qualified(dialect, &old.table)?;
        let original: BTreeMap<_, _> = old
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.as_str(), (i, c)))
            .collect();
        let mut names = BTreeSet::new();
        let mut retained = BTreeSet::new();
        for (c, origin) in self.after.columns.iter().zip(&self.origins) {
            if blank(&c.name) {
                continue;
            }
            ensure!(names.insert(c.name.as_str()), "字段名称重复: {}", c.name);
            if let Some(origin) = origin {
                ensure!(
                    original.contains_key(origin.as_str()) && retained.insert(origin.as_str()),
                    "原始字段标识无效"
                );
            }
        }
        let pk = |columns: &[Column]| {
            columns
                .iter()
                .filter(|c| c.primary_key && !blank(&c.name))
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        };
        let primary_changed = pk(&old.columns) != pk(&self.after.columns);
        let (mut statements, index_adds, index_clauses) =
            indexes::changes(self, dialect, backslash)?;
        let mut clauses: Vec<String> = Vec::new();
        if primary_changed && !pk(&old.columns).is_empty() {
            if matches!(dialect, Dialect::Mysql) {
                clauses.push("DROP PRIMARY KEY".into());
            } else {
                let name = self
                    .before
                    .primary_name
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("主键约束名称未加载"))?;
                statements.push(format!(
                    "ALTER TABLE {table} DROP CONSTRAINT {}",
                    dialect.quote(name)?
                ));
            }
        }
        for c in &old.columns {
            if !retained.contains(c.name.as_str()) {
                let clause = format!("DROP COLUMN {}", dialect.quote(&c.name)?);
                if matches!(dialect, Dialect::Mysql) {
                    clauses.push(clause);
                } else {
                    statements.push(format!("ALTER TABLE {table} {clause}"));
                }
            }
        }
        let old_order: Vec<_> = old
            .columns
            .iter()
            .filter(|c| retained.contains(c.name.as_str()))
            .map(|c| c.name.as_str())
            .collect();
        let new_order: Vec<_> = self.origins.iter().flatten().map(String::as_str).collect();
        let reordered = old_order != new_order;
        let mut previous: Option<&str> = None;
        for (column, origin) in self.after.columns.iter().zip(&self.origins) {
            if blank(&column.name) {
                continue;
            }
            let name = dialect.quote(&column.name)?;
            let position = if let Some(previous) = previous {
                format!(" AFTER {}", dialect.quote(previous)?)
            } else {
                " FIRST".into()
            };
            if let Some(origin) = origin {
                let &(index, before) = original.get(origin.as_str()).unwrap();
                if column.name != *origin {
                    ensure!(
                        !retained.contains(column.name.as_str()),
                        "请分步执行互相占用名称的字段重命名"
                    );
                    let clause = format!("RENAME COLUMN {} TO {name}", dialect.quote(origin)?);
                    if !matches!(dialect, Dialect::Mysql) {
                        statements.push(format!("ALTER TABLE {table} {clause}"));
                    }
                }
                ensure!(
                    column.computed == before.computed,
                    "生成表达式请通过 SQL 修改；当前草稿已保留"
                );
                if matches!(dialect, Dialect::Mysql) {
                    let raw = self
                        .before
                        .raw_columns
                        .get(index)
                        .ok_or_else(|| anyhow::anyhow!("原始列定义未加载"))?;
                    let mut definition = raw.clone();
                    let t = mysql_definition::tokens(&definition, backslash)?;
                    ensure!(!t.is_empty(), "原始列定义为空");
                    definition.replace_range(t[0].range.clone(), &name);
                    if (
                        column.data_type.as_str(),
                        column.length.as_str(),
                        column.scale.as_str(),
                        column.unsigned,
                        column.zerofill,
                    ) != (
                        before.data_type.as_str(),
                        before.length.as_str(),
                        before.scale.as_str(),
                        before.unsigned,
                        before.zerofill,
                    ) {
                        let ty = format!(
                            "{}{}{}",
                            column_type(dialect, column, backslash)?,
                            if column.unsigned { " UNSIGNED" } else { "" },
                            if column.zerofill { " ZEROFILL" } else { "" }
                        );
                        definition = mysql_definition::replace_type(&definition, &ty, backslash)?;
                    }
                    let mut attributes = Vec::new();
                    if column.not_null != before.not_null {
                        attributes.push((
                            "NULL",
                            if column.not_null { "NOT NULL" } else { "NULL" }.to_owned(),
                        ));
                    }
                    if column.auto_increment != before.auto_increment {
                        attributes.push((
                            "AUTO_INCREMENT",
                            if column.auto_increment {
                                "AUTO_INCREMENT"
                            } else {
                                ""
                            }
                            .to_owned(),
                        ));
                    }
                    for (key, prefix, a, b) in [
                        (
                            "CHARACTER",
                            "CHARACTER SET ",
                            &column.charset,
                            &before.charset,
                        ),
                        ("COLLATE", "COLLATE ", &column.collation, &before.collation),
                    ] {
                        if a != b {
                            token(a)?;
                            attributes.push((
                                key,
                                if a.is_empty() {
                                    String::new()
                                } else {
                                    format!("{prefix}{a}")
                                },
                            ));
                        }
                    }
                    if column.default_value != before.default_value {
                        attributes.push((
                            "DEFAULT",
                            if column.default_value.is_empty() {
                                String::new()
                            } else {
                                format!("DEFAULT {}", default(&column.default_value, backslash))
                            },
                        ));
                    }
                    if column.comment != before.comment {
                        attributes.push((
                            "COMMENT",
                            format!("COMMENT {}", literal(&column.comment, backslash)),
                        ));
                    }
                    for (key, value) in attributes {
                        definition = mysql_definition::replace_attribute(
                            &definition,
                            key,
                            &value,
                            backslash,
                        )?;
                    }
                    let changed = column.data_type != before.data_type
                        || column.length != before.length
                        || column.scale != before.scale
                        || column.not_null != before.not_null
                        || column.auto_increment != before.auto_increment
                        || column.default_value != before.default_value
                        || column.comment != before.comment
                        || column.unsigned != before.unsigned
                        || column.zerofill != before.zerofill
                        || column.charset != before.charset
                        || column.collation != before.collation;
                    if column.name != *origin {
                        clauses.push(format!(
                            "CHANGE COLUMN {} {definition}{position}",
                            dialect.quote(origin)?
                        ));
                    } else if changed || reordered {
                        clauses.push(format!("MODIFY COLUMN {definition}{position}"));
                    }
                } else if matches!(dialect, Dialect::Oracle) {
                    // Oracle MODIFY accepts independent attributes. Do not restate
                    // unchanged defaults/identity clauses obtained from the server.
                    ensure!(
                        column.auto_increment == before.auto_increment,
                        "请通过 SQL 修改 Oracle 标识列属性"
                    );
                    if let Some(raw) = self.before.raw_columns.get(index) {
                        let signature: Vec<String> = serde_json::from_str(raw)?;
                        if signature.get(5).is_some_and(|v| v == "YES") {
                            ensure!(
                                (
                                    column.data_type.as_str(),
                                    column.length.as_str(),
                                    column.scale.as_str(),
                                    column.not_null,
                                    column.default_value.as_str()
                                ) == (
                                    before.data_type.as_str(),
                                    before.length.as_str(),
                                    before.scale.as_str(),
                                    before.not_null,
                                    before.default_value.as_str()
                                ),
                                "虚拟字段只能在网格中修改名称和注释，请使用 SQL 修改生成表达式"
                            );
                        }
                    }
                    let mut attributes = Vec::new();
                    if (
                        column.data_type.as_str(),
                        column.length.as_str(),
                        column.scale.as_str(),
                    ) != (
                        before.data_type.as_str(),
                        before.length.as_str(),
                        before.scale.as_str(),
                    ) {
                        attributes.push(column_type(dialect, column, backslash)?);
                    }
                    if column.default_value != before.default_value {
                        attributes.push(format!(
                            "DEFAULT {}",
                            if column.default_value.is_empty() {
                                "NULL".into()
                            } else {
                                default(&column.default_value, backslash)
                            }
                        ));
                    }
                    if column.not_null != before.not_null {
                        attributes.push(if column.not_null { "NOT NULL" } else { "NULL" }.into());
                    }
                    if !attributes.is_empty() {
                        statements.push(format!(
                            "ALTER TABLE {table} MODIFY ({name} {})",
                            attributes.join(" ")
                        ));
                    }
                    if column.comment != before.comment {
                        statements.push(format!(
                            "COMMENT ON COLUMN {table}.{name} IS {}",
                            literal(&column.comment, backslash)
                        ));
                    }
                } else {
                    column.identity_clause(dialect)?;
                    if before.auto_increment && column.auto_increment {
                        ensure!(
                            column.default_value == before.default_value
                                && column.default_expression == before.default_expression,
                            "修改自动递增列默认值前，请先取消自动递增"
                        );
                        ensure!(column.not_null, "自动递增列必须非空");
                    }
                    if before.auto_increment && !column.auto_increment {
                        statements.push(format!(
                            "ALTER TABLE {table} ALTER COLUMN {name} {}",
                            if before.generation == Generation::Serial
                                || before.default_value.starts_with("nextval(")
                            {
                                "DROP DEFAULT"
                            } else {
                                "DROP IDENTITY IF EXISTS"
                            }
                        ));
                    }
                    if (
                        column.data_type.as_str(),
                        column.length.as_str(),
                        column.scale.as_str(),
                    ) != (
                        before.data_type.as_str(),
                        before.length.as_str(),
                        before.scale.as_str(),
                    ) {
                        statements.push(format!(
                            "ALTER TABLE {table} ALTER COLUMN {name} TYPE {} USING {name}::{}",
                            column_type(dialect, column, backslash)?,
                            column.data_type
                        ));
                    }
                    if column.not_null != before.not_null
                        && (column.not_null || !column.auto_increment)
                    {
                        statements.push(format!(
                            "ALTER TABLE {table} ALTER COLUMN {name} {} NOT NULL",
                            if column.not_null { "SET" } else { "DROP" }
                        ));
                    }
                    if column.default_value != before.default_value
                        || column.default_expression != before.default_expression
                    {
                        if !column.auto_increment {
                            statements.push(format!(
                                "ALTER TABLE {table} ALTER COLUMN {name} {}",
                                column.default_sql(dialect, backslash)?.map_or_else(
                                    || "DROP DEFAULT".into(),
                                    |value| format!("SET DEFAULT {value}")
                                )
                            ));
                        } else if !before.auto_increment && !before.default_value.is_empty() {
                            statements.push(format!(
                                "ALTER TABLE {table} ALTER COLUMN {name} DROP DEFAULT"
                            ));
                        }
                    }
                    if !before.auto_increment && column.auto_increment {
                        if !column.not_null && !before.not_null {
                            statements.push(format!(
                                "ALTER TABLE {table} ALTER COLUMN {name} SET NOT NULL"
                            ));
                        }
                        statements.extend(super::generation::postgres_enable(
                            &table, column, backslash,
                        )?);
                    }
                    if column.comment != before.comment {
                        statements.push(format!(
                            "COMMENT ON COLUMN {table}.{name} IS {}",
                            literal(&column.comment, backslash)
                        ));
                    }
                }
            } else {
                let clause = if matches!(dialect, Dialect::Oracle) {
                    format!("ADD ({})", new_column(dialect, column, backslash)?)
                } else {
                    format!("ADD COLUMN {}", new_column(dialect, column, backslash)?)
                };
                if matches!(dialect, Dialect::Mysql) {
                    clauses.push(format!("{clause}{position}"));
                } else {
                    statements.push(format!("ALTER TABLE {table} {clause}"));
                    if !column.comment.is_empty() {
                        statements.push(format!(
                            "COMMENT ON COLUMN {table}.{name} IS {}",
                            literal(&column.comment, backslash)
                        ));
                    }
                }
            }
            previous = Some(&column.name);
        }
        if primary_changed && !pk(&self.after.columns).is_empty() {
            let keys = self
                .after
                .columns
                .iter()
                .filter(|c| c.primary_key && !blank(&c.name))
                .map(|c| dialect.quote(&c.name))
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            let clause = format!("ADD PRIMARY KEY ({keys})");
            if matches!(dialect, Dialect::Mysql) {
                clauses.push(clause);
            } else {
                statements.push(format!("ALTER TABLE {table} {clause}"));
            }
        }
        if self.after.comment != old.comment {
            if matches!(dialect, Dialect::Mysql) {
                clauses.push(format!(
                    "COMMENT={}",
                    literal(&self.after.comment, backslash)
                ));
            } else {
                statements.push(format!(
                    "COMMENT ON TABLE {table} IS {}",
                    literal(&self.after.comment, backslash)
                ));
            }
        }
        if matches!(dialect, Dialect::Mysql) {
            clauses.extend(self.after.options.mysql_clauses(Some(&old.options))?);
        }
        // Index drops must precede column changes, adds follow the new column definitions.
        let (index_drops, index_add_clauses): (Vec<_>, Vec<_>) = index_clauses
            .into_iter()
            .partition(|s| s.starts_with("DROP INDEX"));
        clauses.splice(0..0, index_drops);
        clauses.extend(index_add_clauses);
        statements.extend(index_adds);
        if !clauses.is_empty() {
            statements.insert(0, format!("ALTER TABLE {table} {}", clauses.join(", ")));
        }
        let auxiliary = auxiliary::plans(&self.after.auxiliary, dialect, &old.table)?;
        statements.splice(0..0, auxiliary.drops);
        statements.extend(auxiliary.adds);
        ensure!(
            statements.iter().map(String::len).sum::<usize>() <= 1024 * 1024,
            "变更语句超过 1 MiB"
        );
        Ok(Plan {
            statements,
            reorder_unsupported: reordered && !matches!(dialect, Dialect::Mysql),
        })
    }
}

#[cfg(test)]
mod tests;
