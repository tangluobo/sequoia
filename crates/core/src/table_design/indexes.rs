//! Editable secondary indexes. Opaque server definitions are retained until explicitly removed.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Normal,
    Unique,
    Fulltext,
    Spatial,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Unique => "UNIQUE",
            Self::Fulltext => "FULLTEXT",
            Self::Spatial => "SPATIAL",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    #[serde(default)]
    pub prefix: Option<u32>,
    #[serde(default)]
    pub descending: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    pub name: String,
    pub fields: Vec<Field>,
    pub kind: Kind,
    pub method: String,
    pub comment: String,
    /// Stable identity within the current server snapshot; new rows have no origin.
    pub origin: Option<String>,
    /// Complete catalog signature for optimistic concurrency, never executed as SQL.
    pub signature: String,
    /// Expression, predicate, INCLUDE, custom operator class, visibility, etc.
    pub opaque: String,
    pub constraint: bool,
}
pub(super) fn identity(dialect: Dialect, name: &str) -> String {
    if matches!(dialect, Dialect::Mysql) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}
pub fn kinds(dialect: Dialect) -> &'static [Kind] {
    match dialect {
        Dialect::Mysql => &[Kind::Normal, Kind::Unique, Kind::Fulltext, Kind::Spatial],
        Dialect::Postgres | Dialect::Oracle => &[Kind::Normal, Kind::Unique],
        Dialect::Hive => &[],
    }
}
pub fn methods(dialect: Dialect, kind: Kind) -> &'static [&'static str] {
    match (dialect, kind) {
        (Dialect::Mysql, Kind::Normal | Kind::Unique) => &["BTREE", "HASH"],
        (Dialect::Mysql, _) => &[""],
        (Dialect::Postgres, Kind::Unique) => &["btree"],
        (Dialect::Postgres, _) => &["btree", "hash", "gist", "spgist", "gin", "brin"],
        (Dialect::Oracle, Kind::Unique) => &["BTREE", "REVERSE"],
        (Dialect::Oracle, _) => &["BTREE", "REVERSE", "BITMAP"],
        (Dialect::Hive, _) => &[],
    }
}
impl Index {
    pub fn fields_label(&self) -> String {
        self.fields
            .iter()
            .map(|f| {
                format!(
                    "{}{}{}",
                    f.name,
                    f.prefix.map_or_else(String::new, |n| format!("({n})")),
                    if f.descending { " DESC" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
    pub fn validate(&self, dialect: Dialect, columns: &[Column]) -> Result<()> {
        ensure!(
            !blank(&self.name) && !self.name.contains('\0'),
            "索引名称不能为空或包含空字符"
        );
        ensure!(
            self.name.len() <= 1024 && self.comment.len() <= 64 * 1024,
            "索引名称或注释过长"
        );
        ensure!(
            self.opaque.is_empty() && !self.constraint,
            "索引 {} 含受保护属性，请通过 SQL 修改",
            self.name
        );
        ensure!(
            kinds(dialect).contains(&self.kind),
            "当前数据库不支持此索引类型"
        );
        ensure!(
            methods(dialect, self.kind).contains(&self.method.as_str()),
            "索引 {} 的方法无效",
            self.name
        );
        ensure!(
            !self.fields.is_empty() && self.fields.len() <= 32,
            "索引 {} 请选择 1–32 个字段",
            self.name
        );
        let mut names = BTreeSet::new();
        for f in &self.fields {
            ensure!(
                columns.iter().any(|c| c.name == f.name) && names.insert(&f.name),
                "索引 {} 的字段不存在或重复: {}",
                self.name,
                f.name
            );
            ensure!(
                f.prefix.is_none_or(|n| n > 0
                    && matches!(dialect, Dialect::Mysql)
                    && matches!(self.kind, Kind::Normal | Kind::Unique)),
                "前缀长度仅适用于 MySQL 普通/唯一索引且必须大于 0"
            );
            ensure!(
                !f.descending || !matches!(dialect, Dialect::Mysql) || self.method == "BTREE",
                "降序仅适用于 BTREE 索引"
            );
            ensure!(
                !f.descending || !matches!(dialect, Dialect::Postgres) || self.method == "btree",
                "降序仅适用于 btree 索引"
            );
        }
        ensure!(
            !matches!(dialect, Dialect::Postgres)
                || !["hash", "spgist"].contains(&self.method.as_str())
                || self.fields.len() == 1,
            "此索引方法仅支持单个字段"
        );
        ensure!(
            !matches!(dialect, Dialect::Oracle) || self.comment.is_empty(),
            "Oracle 不支持索引注释"
        );
        Ok(())
    }
    pub fn create_sql(
        &self,
        dialect: Dialect,
        table: &TableId,
        columns: &[Column],
        backslash: bool,
    ) -> Result<String> {
        self.validate(dialect, columns)?;
        let fields = self
            .fields
            .iter()
            .map(|f| {
                Ok(format!(
                    "{}{}{}",
                    dialect.quote(&f.name)?,
                    f.prefix.map_or_else(String::new, |n| format!("({n})")),
                    if f.descending { " DESC" } else { "" }
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        let qualified = format!(
            "{}.{}",
            dialect.quote(&table.schema)?,
            dialect.quote(&table.name)?
        );
        let mut sql = format!(
            "CREATE {}INDEX {} ON {qualified}",
            match self.kind {
                Kind::Normal =>
                    if matches!(dialect, Dialect::Oracle) && self.method == "BITMAP" {
                        "BITMAP "
                    } else {
                        ""
                    },
                Kind::Unique => "UNIQUE ",
                Kind::Fulltext => "FULLTEXT ",
                Kind::Spatial => "SPATIAL ",
            },
            if matches!(dialect, Dialect::Oracle) {
                format!(
                    "{}.{}",
                    dialect.quote(&table.schema)?,
                    dialect.quote(&self.name)?
                )
            } else {
                dialect.quote(&self.name)?
            }
        );
        if matches!(dialect, Dialect::Postgres) {
            sql.push_str(&format!(" USING {}", self.method));
        }
        sql.push_str(&format!(" ({fields})"));
        if matches!(dialect, Dialect::Mysql) {
            if !self.method.is_empty() {
                sql.push_str(&format!(" USING {}", self.method));
            }
            if !self.comment.is_empty() {
                sql.push_str(&format!(" COMMENT {}", literal(&self.comment, backslash)));
            }
        } else if matches!(dialect, Dialect::Oracle) && self.method == "REVERSE" {
            sql.push_str(" REVERSE");
        }
        Ok(sql)
    }
    pub fn comment_sql(
        &self,
        dialect: Dialect,
        table: &TableId,
        backslash: bool,
    ) -> Result<String> {
        Ok(format!(
            "COMMENT ON INDEX {}.{} IS {}",
            dialect.quote(&table.schema)?,
            dialect.quote(&self.name)?,
            literal(&self.comment, backslash)
        ))
    }
}

/// Partition changes around column DDL. MySQL emits one atomic ALTER TABLE.
pub(super) fn changes(
    edit: &alter::Edit,
    dialect: Dialect,
    backslash: bool,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let old = &edit.before.definition;
    let mut after_indexes = edit.after.indexes.clone();
    for index in &mut after_indexes {
        for f in &mut index.fields {
            if let Some((_, c)) = edit
                .origins
                .iter()
                .zip(&edit.after.columns)
                .find(|(o, _)| o.as_ref() == Some(&f.name))
            {
                f.name = c.name.clone();
            }
        }
    }
    let mut drops = Vec::new();
    let mut adds = Vec::new();
    let mut mysql = Vec::new();
    ensure!(edit.after.indexes.len() <= 1024, "索引超过 1,024 项");
    let table = format!(
        "{}.{}",
        dialect.quote(&old.table.schema)?,
        dialect.quote(&old.table.name)?
    );
    let mut names = BTreeSet::new();
    let mut origins = BTreeSet::new();
    for index in &after_indexes {
        ensure!(
            !blank(&index.name) && !index.name.contains('\0'),
            "索引名称不能为空或包含空字符"
        );
        ensure!(
            names.insert(identity(dialect, &index.name)),
            "索引名称重复: {}",
            index.name
        );
        if let Some(origin) = &index.origin {
            ensure!(origins.insert(origin), "原始索引标识重复");
            ensure!(
                old.indexes.iter().any(|i| &i.name == origin),
                "原始索引不存在"
            );
        }
    }
    for before in &old.indexes {
        let after = after_indexes
            .iter()
            .find(|i| i.origin.as_ref() == Some(&before.name));
        let mut mapped = before.clone();
        for f in &mut mapped.fields {
            if let Some((_, c)) = edit
                .origins
                .iter()
                .zip(&edit.after.columns)
                .find(|(o, _)| o.as_ref() == Some(&f.name))
            {
                f.name = c.name.clone();
            }
        }
        let rebuild = after.is_some_and(|i| {
            i.fields != mapped.fields
                || i.kind != before.kind
                || i.method != before.method
                || matches!(dialect, Dialect::Mysql) && i.comment != before.comment
        });
        if let Some(after) = after {
            ensure!(
                after.signature == before.signature
                    && after.opaque == before.opaque
                    && after.constraint == before.constraint,
                "原始索引元数据不一致"
            );
            ensure!(
                !before.constraint || !rebuild,
                "索引 {} 由约束管理，请通过约束 SQL 修改",
                before.name
            );
            ensure!(
                !rebuild || before.opaque.is_empty(),
                "索引 {} 含特殊属性，请通过 SQL 修改，避免丢失原定义",
                before.name
            );
            if before.name != after.name {
                ensure!(
                    !old.indexes
                        .iter()
                        .any(|i| identity(dialect, &i.name) == identity(dialect, &after.name)),
                    "索引重命名目标已存在，请分步重命名"
                );
                if !rebuild {
                    if matches!(dialect, Dialect::Mysql) {
                        mysql.push(format!(
                            "RENAME INDEX {} TO {}",
                            dialect.quote(&before.name)?,
                            dialect.quote(&after.name)?
                        ));
                    } else {
                        adds.push(format!(
                            "ALTER INDEX {}.{} RENAME TO {}",
                            dialect.quote(&old.table.schema)?,
                            dialect.quote(&before.name)?,
                            dialect.quote(&after.name)?
                        ));
                    }
                }
            }
            if !rebuild && after.comment != before.comment && matches!(dialect, Dialect::Postgres) {
                adds.push(after.comment_sql(dialect, &old.table, backslash)?);
            }
        }
        if after.is_none() || rebuild {
            ensure!(
                !before.constraint,
                "索引 {} 由约束管理，不能直接删除",
                before.name
            );
            if matches!(dialect, Dialect::Mysql) {
                mysql.push(format!("DROP INDEX {}", dialect.quote(&before.name)?));
            } else {
                drops.push(format!(
                    "DROP INDEX {}.{}",
                    dialect.quote(&old.table.schema)?,
                    dialect.quote(&before.name)?
                ));
            }
        }
    }
    for index in &after_indexes {
        let before = index
            .origin
            .as_ref()
            .and_then(|o| old.indexes.iter().find(|i| &i.name == o));
        let mut mapped = before.cloned();
        if let Some(b) = &mut mapped {
            for f in &mut b.fields {
                if let Some((_, c)) = edit
                    .origins
                    .iter()
                    .zip(&edit.after.columns)
                    .find(|(o, _)| o.as_ref() == Some(&f.name))
                {
                    f.name = c.name.clone();
                }
            }
        }
        let rebuild = mapped.as_ref().is_none_or(|b| {
            b.fields != index.fields
                || b.kind != index.kind
                || b.method != index.method
                || matches!(dialect, Dialect::Mysql) && b.comment != index.comment
        });
        if rebuild {
            let sql = index.create_sql(dialect, &old.table, &edit.after.columns, backslash)?;
            if matches!(dialect, Dialect::Mysql) {
                // Transform generated CREATE INDEX into an ALTER clause, never parse user SQL.
                let prefix = format!(
                    "CREATE {}INDEX {} ON {table}",
                    if index.kind == Kind::Normal {
                        String::new()
                    } else {
                        format!("{} ", index.kind.label())
                    },
                    dialect.quote(&index.name)?
                );
                mysql.push(format!(
                    "ADD {}INDEX {}{}",
                    if index.kind == Kind::Normal {
                        String::new()
                    } else {
                        format!("{} ", index.kind.label())
                    },
                    dialect.quote(&index.name)?,
                    sql.strip_prefix(&prefix)
                        .ok_or_else(|| anyhow::anyhow!("索引语句生成失败"))?
                ));
            } else {
                adds.push(sql);
                if matches!(dialect, Dialect::Postgres) && !index.comment.is_empty() {
                    adds.push(index.comment_sql(dialect, &old.table, backslash)?);
                }
            }
        }
    }
    Ok((drops, adds, mysql))
}

#[cfg(test)]
mod tests;
