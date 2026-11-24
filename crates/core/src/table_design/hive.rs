//! Hive's ADD/CHANGE COLUMNS operations preserve storage and partition options.
use super::*;
use alter::{Edit, Plan};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn safe_type(value: &str) -> Result<()> {
    ensure!(
        !blank(value) && value.len() <= 64 * 1024,
        "Hive 字段类型无效"
    );
    let mut nesting = Vec::new();
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            if quoted && chars.peek() == Some(&'`') {
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if !quoted {
            match ch {
                '<' | '(' => {
                    ensure!(nesting.len() < 64, "Hive 字段类型嵌套过深");
                    nesting.push(ch);
                }
                '>' | ')' => ensure!(
                    nesting.pop() == Some(if ch == '>' { '<' } else { '(' }),
                    "Hive 字段类型括号不匹配"
                ),
                ',' | ':' => ensure!(!nesting.is_empty(), "Hive 字段类型无效"),
                _ => ensure!(
                    ch.is_alphanumeric() || ch == '_' || ch == ' ' || ch == '\t',
                    "Hive 字段类型无效"
                ),
            }
        } else {
            ensure!(!ch.is_control(), "Hive 字段类型无效");
        }
    }
    ensure!(
        !quoted && nesting.is_empty(),
        "Hive 字段类型括号或字段名未闭合"
    );
    Ok(())
}
fn constraints(column: &Column) -> (&str, bool, bool, bool, bool, bool, &str, &str, &str, bool) {
    (
        &column.default_value,
        column.not_null,
        column.primary_key,
        column.auto_increment,
        column.unsigned,
        column.zerofill,
        &column.charset,
        &column.collation,
        &column.key_length,
        column.binary,
    )
}
fn clause(column: &Column) -> Result<String> {
    let mut value = format!(
        "{} {}",
        Dialect::Hive.quote(&column.name)?,
        declaration(Dialect::Hive, column, false)?
    );
    if !column.comment.is_empty() {
        value.push_str(&format!(" COMMENT {}", literal(&column.comment, false)));
    }
    Ok(value)
}
pub(super) fn plan(edit: &Edit) -> Result<Plan> {
    edit.after.validate_input()?;
    ensure!(!edit.before.view, "视图字段不能通过 ALTER TABLE 修改");
    let old = &edit.before.definition;
    ensure!(
        old.table == edit.after.table && edit.origins.len() == edit.after.columns.len(),
        "表或字段标识不一致"
    );
    ensure!(
        old.indexes == edit.after.indexes && old.options == edit.after.options,
        "Hive 索引及存储选项请通过 SQL 编辑器修改"
    );
    let table = format!(
        "{}.{}",
        Dialect::Hive.quote(&old.table.schema)?,
        Dialect::Hive.quote(&old.table.name)?
    );
    let original: BTreeMap<_, _> = old.columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut retained = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut columns = Vec::new();
    for (column, origin) in edit
        .after
        .columns
        .iter()
        .zip(&edit.origins)
        .filter(|(c, _)| !blank(&c.name))
    {
        ensure!(
            names.insert(column.name.as_str()),
            "字段名称重复: {}",
            column.name
        );
        clause(column)?;
        if let Some(origin) = origin {
            let before = original
                .get(origin.as_str())
                .ok_or_else(|| anyhow::anyhow!("原始字段标识无效"))?;
            ensure!(retained.insert(origin.as_str()), "原始字段标识重复");
            ensure!(
                constraints(column) == constraints(before),
                "Hive 字段约束请通过 SQL 编辑器修改"
            );
        } else {
            ensure!(
                constraints(column) == constraints(&Column::default()),
                "Hive 新字段约束请通过 SQL 编辑器修改"
            );
        }
        columns.push((column, origin));
    }
    ensure!(
        retained.len() == old.columns.len(),
        "Hive 删除字段需要根据完整字段列表执行 REPLACE COLUMNS，请使用 SQL 编辑器确认后执行"
    );
    let mut statements = Vec::new();
    let mut occupied: BTreeSet<String> = original
        .keys()
        .map(|v| (*v).into())
        .chain(names.iter().map(|v| (*v).into()))
        .collect();
    let mut temporary = BTreeMap::new();
    // Rename through temporary names, allowing A/B swaps without colliding.
    for (index, (column, origin)) in columns.iter().enumerate() {
        if let Some(origin) = origin
            && **origin != column.name
        {
            let mut suffix = index;
            let name = loop {
                let name = format!("_tomato_rename_{suffix}");
                if occupied.insert(name.clone()) {
                    break name;
                }
                suffix += 1;
            };
            let mut before = (*original[origin.as_str()]).clone();
            before.name = name.clone();
            statements.push(format!(
                "ALTER TABLE {table} CHANGE COLUMN {} {}",
                Dialect::Hive.quote(origin)?,
                clause(&before)?
            ));
            temporary.insert(origin.as_str(), name);
        }
    }
    for (column, origin) in &columns {
        if let Some(origin) = origin {
            if *column != original[origin.as_str()] {
                statements.push(format!(
                    "ALTER TABLE {table} CHANGE COLUMN {} {}",
                    Dialect::Hive.quote(
                        temporary
                            .get(origin.as_str())
                            .map_or(origin.as_str(), String::as_str)
                    )?,
                    clause(column)?
                ));
            }
        } else {
            statements.push(format!(
                "ALTER TABLE {table} ADD COLUMNS ({})",
                clause(column)?
            ));
        }
    }
    if edit.after.comment != old.comment {
        statements.push(format!(
            "ALTER TABLE {table} SET TBLPROPERTIES ('comment'={})",
            literal(&edit.after.comment, false)
        ));
    }
    let actual_order: Vec<_> = old
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .chain(
            columns
                .iter()
                .filter(|(_, o)| o.is_none())
                .map(|(c, _)| c.name.as_str()),
        )
        .collect();
    let desired: Vec<_> = columns
        .iter()
        .map(|(c, o)| o.as_deref().unwrap_or(&c.name))
        .collect();
    Ok(Plan {
        statements,
        reorder_unsupported: actual_order != desired,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hive_changes_preserve_complex_types_and_make_rename_swaps_possible() -> Result<()> {
        let column = |name: &str| Column {
            name: name.into(),
            data_type: "STRUCT<`中文字段`:MAP<STRING,DECIMAL(12,2)>,tag:ARRAY<STRING>>".into(),
            ..Default::default()
        };
        let before = Create {
            auxiliary: Vec::new(),
            table: TableId {
                schema: "db`中文".into(),
                name: "items".into(),
            },
            columns: vec![column("a"), column("b")],
            indexes: vec![],
            options: Options::default(),
            comment: String::new(),
        };
        let mut after = before.clone();
        after.columns.swap(0, 1);
        let edit = Edit {
            before: alter::Baseline {
                definition: before.clone(),
                raw_columns: vec![],
                primary_name: None,
                view: false,
            },
            after,
            origins: vec![Some("a".into()), Some("b".into())],
        };
        let actual = edit.plan(Dialect::Hive, false)?;
        assert_eq!(actual.statements.len(), 4);
        assert!(actual.statements[0].starts_with(
            "ALTER TABLE `db``中文`.`items` CHANGE COLUMN `a` `_tomato_rename_0` STRUCT<"
        ));
        assert!(actual.statements[2].contains("CHANGE COLUMN `_tomato_rename_0` `b` STRUCT<"));
        assert!(!actual.reorder_unsupported);
        for invalid in [
            "STRING;DROP TABLE x",
            "MAP<STRING,INT",
            "INT)",
            "INT,STRING",
            "ARRAY<INT> -- comment",
        ] {
            assert!(safe_type(invalid).is_err(), "{invalid}");
        }
        assert!(safe_type("STRUCT<`a``b`:MAP<STRING,INT>>").is_ok());
        Ok(())
    }
    #[test]
    fn hive_deleted_fields_are_rejected_before_any_sql_is_sent() {
        let before = Create {
            auxiliary: Vec::new(),
            table: TableId {
                schema: "default".into(),
                name: "items".into(),
            },
            columns: vec![Column {
                name: "id".into(),
                data_type: "INT".into(),
                ..Default::default()
            }],
            indexes: vec![],
            options: Options::default(),
            comment: String::new(),
        };
        let mut after = before.clone();
        after.columns.clear();
        let edit = Edit {
            before: alter::Baseline {
                definition: before,
                raw_columns: vec![],
                primary_name: None,
                view: false,
            },
            after,
            origins: vec![],
        };
        assert!(
            edit.plan(Dialect::Hive, false)
                .unwrap_err()
                .to_string()
                .contains("REPLACE COLUMNS")
        );
    }
}
