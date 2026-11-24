use super::*;
#[test]
fn oracle_plan_preserves_unedited_defaults_and_character_semantics() -> Result<()> {
    let definition = Create {
        auxiliary: Vec::new(),
        indexes: Vec::new(),
        table: TableId {
            schema: "用户\"甲".into(),
            name: "items".into(),
        },
        columns: vec![
            Column {
                name: "id".into(),
                data_type: "NUMBER".into(),
                primary_key: true,
                not_null: true,
                ..Default::default()
            },
            Column {
                name: "value".into(),
                data_type: "VARCHAR2".into(),
                length: "10".into(),
                charset: "CHAR".into(),
                default_value: "SYS_GUID()".into(),
                ..Default::default()
            },
            Column {
                name: "stamp".into(),
                data_type: "TIMESTAMP(6) WITH TIME ZONE".into(),
                scale: "6".into(),
                ..Default::default()
            },
        ],
        options: Options::default(),
        comment: String::new(),
    };
    let mut after = definition.clone();
    after.columns[1].name = "值".into();
    after.columns[1].length = "20".into();
    after.columns[1].comment = "it's a value".into();
    after.columns[2].scale = "9".into();
    after.columns.push(Column {
        name: "n".into(),
        data_type: "NUMBER".into(),
        length: "10".into(),
        scale: "-2".into(),
        not_null: true,
        default_value: "NULL".into(),
        ..Default::default()
    });
    let edit = Edit {
        before: Baseline {
            definition,
            raw_columns: vec![],
            primary_name: Some("SYS_PK".into()),
            view: false,
        },
        after,
        origins: vec![
            Some("id".into()),
            Some("value".into()),
            Some("stamp".into()),
            None,
        ],
    };
    let sql = edit.plan(Dialect::Oracle, false)?.statements;
    assert_eq!(
        sql,
        vec![
            "ALTER TABLE \"用户\"\"甲\".\"items\" RENAME COLUMN \"value\" TO \"值\"",
            "ALTER TABLE \"用户\"\"甲\".\"items\" MODIFY (\"值\" VARCHAR2(20 CHAR))",
            "COMMENT ON COLUMN \"用户\"\"甲\".\"items\".\"值\" IS 'it''s a value'",
            "ALTER TABLE \"用户\"\"甲\".\"items\" MODIFY (\"stamp\" TIMESTAMP(9) WITH TIME ZONE)",
            "ALTER TABLE \"用户\"\"甲\".\"items\" ADD (\"n\" NUMBER(10,-2) DEFAULT NULL NOT NULL)",
        ]
    );
    assert!(
        !sql.iter()
            .any(|s| s.contains("SYS_GUID") || s.contains("ALTER COLUMN"))
    );
    Ok(())
}
#[test]
fn mysql_preserves_quoted_commas_generated_expressions_and_untouched_attributes() -> Result<()> {
    let create = "CREATE TABLE `a``b` (`id` int NOT NULL, `v` enum('a,b','x\\\'y') DEFAULT 'a,b' COMMENT 'old', `stamp` timestamp(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6), `g` int GENERATED ALWAYS AS ((`id` + 1)) STORED INVISIBLE, PRIMARY KEY (`id`)) ENGINE=InnoDB";
    let columns = mysql_definition::columns(create, true)?;
    assert_eq!(columns.len(), 4);
    let stamp = &columns[2].1;
    let changed = mysql_definition::replace_attribute(stamp, "COMMENT", "COMMENT 'new'", true)?;
    assert_eq!(changed, format!("{stamp} COMMENT 'new'"));
    let changed = mysql_definition::replace_attribute(&changed, "DEFAULT", "DEFAULT NULL", true)?;
    assert!(changed.contains("DEFAULT NULL ON UPDATE CURRENT_TIMESTAMP(6) COMMENT 'new'"));
    let changed = mysql_definition::replace_attribute(&changed, "NULL", "NULL", true)?;
    assert!(!changed.contains("NOT NULL"));
    assert!(changed.contains("DEFAULT NULL"));
    let generated = mysql_definition::replace_type(&columns[3].1, "BIGINT", true)?;
    assert!(generated.contains("BIGINT GENERATED ALWAYS AS ((`id` + 1)) STORED INVISIBLE"));
    assert!(mysql_definition::tokens("`a` enum('x'", true).is_err());
    Ok(())
}
