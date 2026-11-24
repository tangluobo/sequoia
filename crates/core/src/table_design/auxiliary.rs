//! Staged foreign keys and triggers. Metadata is never executed unless explicitly changed.
use super::{alter::InfoPage, *};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    #[serde(default)]
    pub before_signature: String,
    pub page: InfoPage,
    pub before: Vec<Vec<String>>,
    pub after: Vec<Vec<String>>,
}
pub struct Plan {
    pub drops: Vec<String>,
    pub adds: Vec<String>,
}
pub fn headers(page: InfoPage) -> Vec<String> {
    match page {
        InfoPage::ForeignKeys => [
            "名称",
            "字段",
            "参考数据库",
            "参考表",
            "参考字段",
            "删除时",
            "更新时",
        ]
        .as_slice(),
        InfoPage::Triggers => ["名称", "时机", "事件", "语句"].as_slice(),
        _ => [].as_slice(),
    }
    .iter()
    .map(|s| (*s).into())
    .collect()
}
pub fn default_row(page: InfoPage, schema: &str) -> Vec<String> {
    let mut values = match page {
        InfoPage::ForeignKeys => vec!["", "", "", "", "", "NO ACTION", "NO ACTION"],
        InfoPage::Triggers => vec!["", "BEFORE", "INSERT", ""],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if page == InfoPage::ForeignKeys {
        values[2] = schema.into();
    }
    values
}
/// Comma-separated identifiers, with doubled SQL quotes for names containing a comma.
pub fn fields_label(fields: &[String]) -> String {
    let quoted = |name: &str| name.trim() != name || name.contains([',', '"', '`']);
    let capacity = fields
        .iter()
        .map(|name| {
            name.len()
                + if quoted(name) {
                    name.matches('"').count() + 2
                } else {
                    0
                }
        })
        .sum::<usize>()
        + fields.len().saturating_sub(1);
    let mut label = String::with_capacity(capacity);
    for (index, name) in fields.iter().enumerate() {
        if index > 0 {
            label.push(',');
        }
        if quoted(name) {
            label.push('"');
            for character in name.chars() {
                if character == '"' {
                    label.push('"');
                }
                label.push(character);
            }
            label.push('"');
        } else {
            label.push_str(name);
        }
    }
    label
}

/// Parse the editable field list without splitting quoted identifier contents.
fn fields(text: &str, dialect: Dialect) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut chars = text.trim().chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if let Some(quote @ ('`' | '"')) = chars.peek().copied() {
            chars.next();
            loop {
                let ch = chars
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("字段名称的引号未闭合"))?;
                if ch == quote {
                    if chars.peek() == Some(&quote) {
                        chars.next();
                        value.push(ch);
                    } else {
                        break;
                    }
                } else {
                    value.push(ch);
                }
            }
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
        } else {
            while chars.peek().is_some_and(|c| *c != ',') {
                value.push(chars.next().unwrap());
            }
            value = value.trim().into();
        }
        ensure!(!value.is_empty() && !value.contains('\0'), "请填写字段名称");
        result.push(dialect.quote(&value)?);
        ensure!(result.len() <= 64, "外键字段超过 64 项");
        match chars.next() {
            None => break,
            Some(',') => ensure!(chars.peek().is_some(), "字段列表不能以逗号结尾"),
            _ => bail!("字段名称之间请使用逗号分隔"),
        }
    }
    Ok(result)
}
fn rule(text: &str, dialect: Dialect, update: bool) -> Result<String> {
    let rule = text.trim().to_ascii_uppercase();
    ensure!(
        [
            "NO ACTION",
            "RESTRICT",
            "CASCADE",
            "SET NULL",
            "SET DEFAULT"
        ]
        .contains(&rule.as_str()),
        "外键操作无效"
    );
    if matches!(dialect, Dialect::Oracle) {
        ensure!(
            !update || rule == "NO ACTION",
            "Oracle 不支持 ON UPDATE 外键操作"
        );
        ensure!(
            ["NO ACTION", "CASCADE", "SET NULL"].contains(&rule.as_str()),
            "Oracle 不支持此删除操作"
        );
    }
    ensure!(
        !matches!(dialect, Dialect::Mysql) || rule != "SET DEFAULT",
        "MySQL 不支持 SET DEFAULT 外键操作"
    );
    Ok(rule)
}
impl Edit {
    pub fn plan(&self, dialect: Dialect, table: &TableId) -> Result<Plan> {
        self.project(dialect, table, false)
    }
    pub(super) fn creation(&self, dialect: Dialect, table: &TableId) -> Result<Plan> {
        self.project(dialect, table, true)
    }
    fn project(&self, dialect: Dialect, table: &TableId, creation: bool) -> Result<Plan> {
        let before_rows = if creation {
            &[][..]
        } else {
            self.before.as_slice()
        };
        ensure!(
            !matches!(dialect, Dialect::Hive),
            "Hive 不支持外键或触发器设计"
        );
        ensure!(
            !matches!(self.page, InfoPage::Indexes),
            "索引应使用索引设计器"
        );
        ensure!(
            before_rows.len() <= 10_000 && self.after.len() <= 10_000,
            "外键或触发器超过 10,000 项"
        );
        ensure!(self.before_signature.len() <= 128, "结构校验标识过长");
        let columns = if self.page == InfoPage::ForeignKeys {
            7
        } else {
            4
        };
        let collect = |rows: &[Vec<String>]| -> Result<BTreeMap<String, usize>> {
            let mut names = BTreeMap::new();
            for (i, row) in rows.iter().enumerate() {
                ensure!(
                    row.len() == columns && row.iter().all(|s| !s.contains('\0')),
                    "外键或触发器数据不完整"
                );

                dialect.quote(&row[0])?;
                ensure!(
                    names
                        .insert(indexes::identity(dialect, &row[0]), i)
                        .is_none(),
                    "名称重复：{}",
                    row[0]
                );
            }
            Ok(names)
        };
        let before = collect(before_rows)?;
        let after = collect(&self.after)?;
        let table_sql = format!(
            "{}.{}",
            dialect.quote(&table.schema)?,
            dialect.quote(&table.name)?
        );
        let mut plan = Plan {
            drops: Vec::new(),
            adds: Vec::new(),
        };
        for (key, i) in &before {
            let row = &before_rows[*i];
            if after.get(key).is_some_and(|j| self.after[*j] == *row) {
                continue;
            }
            let name = dialect.quote(&row[0])?;
            plan.drops.push(match self.page {
                InfoPage::ForeignKeys => format!(
                    "ALTER TABLE {table_sql} DROP {} {name}",
                    if matches!(dialect, Dialect::Mysql) {
                        "FOREIGN KEY"
                    } else {
                        "CONSTRAINT"
                    }
                ),
                _ => match dialect {
                    Dialect::Postgres => format!("DROP TRIGGER {name} ON {table_sql}"),
                    _ => format!("DROP TRIGGER {}.{name}", dialect.quote(&table.schema)?),
                },
            });
        }
        for (key, i) in &after {
            let row = &self.after[*i];
            if before.get(key).is_some_and(|j| before_rows[*j] == *row) {
                continue;
            }
            let name = dialect.quote(&row[0])?;
            if self.page == InfoPage::ForeignKeys {
                let local = fields(&row[1], dialect)?;
                let remote = fields(&row[4], dialect)?;
                ensure!(
                    local.len() == remote.len(),
                    "本表字段与参考字段的数量必须相同"
                );
                let reference = format!("{}.{}", dialect.quote(&row[2])?, dialect.quote(&row[3])?);
                let delete = rule(&row[5], dialect, false)?;
                let update = rule(&row[6], dialect, true)?;
                let mut sql = format!(
                    "ALTER TABLE {table_sql} ADD CONSTRAINT {name} FOREIGN KEY ({}) REFERENCES {reference} ({})",
                    local.join(", "),
                    remote.join(", ")
                );
                if !matches!(dialect, Dialect::Oracle) || delete != "NO ACTION" {
                    sql.push_str(&format!(" ON DELETE {delete}"));
                }
                if !matches!(dialect, Dialect::Oracle) {
                    sql.push_str(&format!(" ON UPDATE {update}"));
                }
                plan.adds.push(sql);
            } else {
                let timing = row[1].trim().to_ascii_uppercase();
                ensure!(
                    ["BEFORE", "AFTER", "INSTEAD OF"].contains(&timing.as_str()),
                    "触发时机无效"
                );
                ensure!(
                    !matches!(dialect, Dialect::Mysql) || timing != "INSTEAD OF",
                    "MySQL 不支持 INSTEAD OF 触发器"
                );
                let events = row[2]
                    .split(" OR ")
                    .map(|e| e.trim().to_ascii_uppercase())
                    .collect::<Vec<_>>();
                ensure!(
                    !events.is_empty()
                        && events.len() <= 4
                        && events
                            .iter()
                            .all(|e| ["INSERT", "UPDATE", "DELETE", "TRUNCATE"]
                                .contains(&e.as_str())),
                    "触发事件无效"
                );
                ensure!(
                    !matches!(dialect, Dialect::Mysql) || events.len() == 1,
                    "MySQL 每个触发器只能指定一个事件"
                );
                ensure!(
                    matches!(dialect, Dialect::Postgres) || !events.iter().any(|e| e == "TRUNCATE"),
                    "当前数据库不支持 TRUNCATE 触发事件"
                );
                let body = row[3].trim();
                // END; belongs to an Oracle PL/SQL unit. Removing it makes a
                // previously valid trigger fail when the actor executes it.
                let body = if matches!(dialect, Dialect::Oracle) {
                    body
                } else {
                    body.trim_end_matches(';').trim_end()
                };
                ensure!(!body.is_empty(), "请填写触发器语句或触发函数");
                let name = if matches!(dialect, Dialect::Postgres) {
                    name
                } else {
                    format!("{}.{name}", dialect.quote(&table.schema)?)
                };
                if matches!(dialect, Dialect::Postgres) {
                    ensure!(
                        !body.contains(';')
                            && !body.contains("--")
                            && !body.contains("/*")
                            && body.contains('(')
                            && body.ends_with(')'),
                        "请填写触发函数调用，例如 public.audit_row()，不要填写完整 CREATE TRIGGER 语句"
                    );
                    plan.adds.push(format!("CREATE TRIGGER {name} {timing} {} ON {table_sql} FOR EACH {} EXECUTE FUNCTION {body}",events.join(" OR "),if events.iter().any(|e| e == "TRUNCATE") {"STATEMENT"} else {"ROW"}));
                } else {
                    plan.adds.push(format!(
                        "CREATE TRIGGER {name} {timing} {} ON {table_sql} FOR EACH ROW {body}",
                        events.join(" OR ")
                    ));
                }
            }
        }
        Ok(plan)
    }
}

pub(super) fn plans(edits: &[Edit], dialect: Dialect, table: &TableId) -> Result<Plan> {
    ensure!(edits.len() <= 2, "外键或触发器变更重复");
    let mut seen = Vec::new();
    let mut plan = Plan {
        drops: Vec::new(),
        adds: Vec::new(),
    };
    for edit in edits {
        ensure!(!seen.contains(&edit.page), "外键或触发器变更重复");
        seen.push(edit.page);
        let next = edit.plan(dialect, table)?;
        plan.drops.extend(next.drops);
        plan.adds.extend(next.adds);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_field_labels_roundtrip_to_ddl_for_every_supported_foreign_key_dialect() -> Result<()>
    {
        let names = ["id", "x,y", "a\"b", " x ", "`x", "中文字段"]
            .map(String::from)
            .to_vec();
        let label = fields_label(&names);
        assert_eq!(fields_label(&["id".into(), "name".into()]), "id,name");
        assert_eq!(fields_label(&[]), "");
        for dialect in [Dialect::Mysql, Dialect::Postgres, Dialect::Oracle] {
            let expected = names
                .iter()
                .map(|name| dialect.quote(name))
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(fields(&label, dialect)?, expected);
            let table = TableId {
                schema: "owned".into(),
                name: "items".into(),
            };
            let mut row = default_row(InfoPage::ForeignKeys, "owned");
            row[0] = "fk_names".into();
            row[1] = label.clone();
            row[3] = "parent".into();
            row[4] = label.clone();
            let plan = Edit {
                before_signature: String::new(),
                page: InfoPage::ForeignKeys,
                before: vec![],
                after: vec![row],
            }
            .plan(dialect, &table)?;
            assert!(plan.adds[0].contains(&format!("FOREIGN KEY ({})", expected.join(", "))));
        }
        Ok(())
    }
    #[test]
    fn untouched_server_trigger_statements_are_never_replayed() -> Result<()> {
        let row = vec![
            "t".into(),
            "AFTER".into(),
            "INSERT".into(),
            "opaque server definition; DROP TABLE unrelated".into(),
        ];
        let edit = Edit {
            before_signature: String::new(),
            page: InfoPage::Triggers,
            before: vec![row.clone()],
            after: vec![row],
        };
        let table = TableId {
            schema: "owned".into(),
            name: "items".into(),
        };
        let plan = edit.plan(Dialect::Postgres, &table)?;
        assert!(plan.drops.is_empty() && plan.adds.is_empty());
        Ok(())
    }
    #[test]
    fn composite_identifier_lists_support_escaped_commas_and_reject_partial_pairs() -> Result<()> {
        let table = TableId {
            schema: "owned".into(),
            name: "items".into(),
        };
        let mut row = default_row(InfoPage::ForeignKeys, "remote");
        row[0] = "fk\"name".into();
        row[1] = "\"a,b\", \"c\"\"d\"".into();
        row[3] = "parent".into();
        row[4] = "x,y".into();
        let mut edit = Edit {
            before_signature: String::new(),
            page: InfoPage::ForeignKeys,
            before: vec![],
            after: vec![row],
        };
        let plan = edit.plan(Dialect::Postgres, &table)?;
        assert!(plan.adds[0].contains("FOREIGN KEY (\"a,b\", \"c\"\"d\")"));
        edit.after[0][4] = "x".into();
        assert!(edit.plan(Dialect::Postgres, &table).is_err());
        edit.after[0][4] = "x,y,".into();
        assert!(edit.plan(Dialect::Postgres, &table).is_err());
        Ok(())
    }
}
