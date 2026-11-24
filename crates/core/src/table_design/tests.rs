use super::*;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Original {
    name: String,
    kind: Dialect,
    database: String,
    schema: Option<String>,
    table: String,
    columns: Vec<HashMap<String, String>>,
    options: Option<HashMap<String, String>>,
    comment: String,
    sql: String,
}
#[test]
fn legacy_captures_and_documented_native_identity_corrections_are_explicit() -> Result<()> {
    let rows: Vec<Original> = serde_json::from_str(include_str!(
        "../../tests/fixtures/table-design/original-create.json"
    ))?;
    assert_eq!(rows.len(), 9);
    for row in rows {
        let columns = row
            .columns
            .into_iter()
            .map(|column| {
                let text = |name: &str| column.get(name).cloned().unwrap_or_default();
                let flag = |name: &str| column.get(name).is_some_and(|value| value == "是");
                Column {
                    computed: None,
                    generation: Default::default(),
                    default_expression: false,
                    name: text("字段名"),
                    data_type: text("类型"),
                    length: text("长度"),
                    scale: text("小数点"),
                    not_null: flag("非空"),
                    primary_key: flag("主键"),
                    auto_increment: flag("自增"),
                    default_value: text("默认值"),
                    comment: text("注释"),
                    unsigned: flag("无符号"),
                    zerofill: flag("填充零"),
                    charset: text("字符集"),
                    collation: text("排序规则"),
                    key_length: text("键长度"),
                    binary: flag("二进制"),
                }
            })
            .collect();
        let options = row.options.unwrap_or_default();
        let create = Create {
            auxiliary: Vec::new(),
            indexes: Vec::new(),
            table: TableId {
                schema: row.schema.unwrap_or(row.database),
                name: row.table,
            },
            columns,
            options: Options {
                engine: options.get("引擎").cloned().unwrap_or_default(),
                charset: options.get("字符集").cloned().unwrap_or_default(),
                collation: options.get("排序规则").cloned().unwrap_or_default(),
                ..Default::default()
            },
            comment: row.comment,
        };
        let corrections: HashMap<String, Option<String>> = serde_json::from_str(include_str!(
            "../../tests/fixtures/table-design/native-create-corrections.json"
        ))?;
        match corrections.get(&row.name) {
            Some(None) => assert!(
                create.sql(row.kind).is_err(),
                "original case {} silently ignored invalid identity settings",
                row.name
            ),
            Some(Some(sql)) => assert_eq!(&create.sql(row.kind)?, sql, "{}", row.name),
            None => assert_eq!(create.sql(row.kind)?, row.sql, "{}", row.name),
        }
    }
    Ok(())
}
fn draft() -> Create {
    Create {
        auxiliary: Vec::new(),
        indexes: Vec::new(),
        table: TableId {
            schema: "sample_db".into(),
            name: "orders".into(),
        },
        columns: vec![Column {
            name: "id".into(),
            data_type: "INT".into(),
            ..Default::default()
        }],
        options: Default::default(),
        comment: String::new(),
    }
}
#[test]
fn create_drafts_quote_identifiers_honor_mysql_literal_mode_and_reject_invalid_fragments()
-> Result<()> {
    let mut create = draft();
    create.table.name = "odd`\"table".into();
    create.columns[0].name = "odd`\"field".into();
    create.columns[0].default_value = r"C:\a'\b".into();
    let mysql = create.sql_for_session(Dialect::Mysql, true)?;
    assert!(mysql.contains("`odd``\"table`"));
    assert!(mysql.contains(r"DEFAULT 'C:\\a''\\b'"));
    assert!(create.sql(Dialect::Mysql)?.contains(r"DEFAULT 'C:\a''\b'"));
    let postgres = create.sql(Dialect::Postgres)?;
    assert!(postgres.contains("\"odd`\"\"table\""));
    assert!(postgres.contains(r"DEFAULT 'C:\a''\b'"));
    assert!(
        create
            .sql_for_session(Dialect::Postgres, true)?
            .contains(r"DEFAULT 'C:\\a''\\b'")
    );
    create.columns[0].data_type = "INT); DROP TABLE users; --".into();
    assert!(create.sql(Dialect::Mysql).is_err());
    create.columns[0].data_type = "INT".into();
    create.options.engine = "InnoDB;DROP TABLE users".into();
    assert!(create.sql(Dialect::Mysql).is_err());
    create.options.engine.clear();
    create.comment = "x".repeat(512 * 1024);
    assert!(create.sql(Dialect::Mysql).is_err());
    assert!(Dialect::resolve("SQLITE").is_err());
    Ok(())
}
#[test]
fn create_validation_uses_original_row_numbers_type_rules_and_java_trim() {
    let mut create = draft();
    create.columns = vec![
        Column {
            name: " ".into(),
            ..Default::default()
        },
        Column {
            name: "title".into(),
            data_type: "VARCHAR".into(),
            ..Default::default()
        },
        Column {
            name: "amount".into(),
            data_type: "DECIMAL".into(),
            length: "8".into(),
            ..Default::default()
        },
        Column {
            name: "\u{2003}".into(),
            ..Default::default()
        },
    ];
    assert_eq!(
        create.validation_errors(),
        vec![
            "第2行字段\"title\"：类型\"VARCHAR\"需要指定长度",
            "第3行字段\"amount\"：类型\"DECIMAL\"需要指定小数点",
            "第4行字段\"\u{2003}\"：未设置类型",
        ]
    );
}
