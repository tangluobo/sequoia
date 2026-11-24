use super::*;
use crate::table_design::alter::{Baseline, Edit};
fn definition(dialect: Dialect) -> Create {
    Create {
        auxiliary: Vec::new(),
        table: TableId {
            schema: "owned".into(),
            name: "items".into(),
        },
        columns: ["id", "label"]
            .into_iter()
            .map(|name| Column {
                name: name.into(),
                data_type: "INT".into(),
                ..Default::default()
            })
            .collect(),
        indexes: vec![Index {
            name: "ix_label".into(),
            origin: Some("ix_label".into()),
            method: methods(dialect, Kind::Normal)[0].into(),
            fields: vec![Field {
                name: "label".into(),
                ..Default::default()
            }],
            signature: "catalog".into(),
            ..Default::default()
        }],
        options: Default::default(),
        comment: String::new(),
    }
}
fn edit(dialect: Dialect) -> Edit {
    let definition = definition(dialect);
    Edit {
        after: definition.clone(),
        origins: vec![Some("id".into()), Some("label".into())],
        before: Baseline {
            definition,
            raw_columns: if matches!(dialect, Dialect::Oracle) {
                vec!["[]".into(), "[]".into()]
            } else {
                vec![
                    "`id` int DEFAULT NULL".into(),
                    "`label` int DEFAULT NULL".into(),
                ]
            },
            primary_name: None,
            view: false,
        },
    }
}
#[test]
fn index_rename_avoids_rebuild_and_column_rename_retains_identity() -> Result<()> {
    for dialect in [Dialect::Mysql, Dialect::Postgres, Dialect::Oracle] {
        let mut edit = edit(dialect);
        assert!(edit.plan(dialect, false)?.statements.is_empty());
        edit.after.indexes[0].name = "ix_new".into();
        let sql = edit.plan(dialect, false)?.statements.join(";");
        assert!(sql.contains("RENAME"));
        assert!(!sql.contains("DROP"));
        assert!(!sql.contains("CREATE"));
        edit.after.columns[1].name = "renamed".into();
        let sql = edit.plan(dialect, false)?.statements.join(";");
        assert!(!sql.contains("DROP INDEX"));
        assert!(!sql.contains("CREATE INDEX"));
    }
    Ok(())
}
#[test]
fn index_type_method_order_and_comments_generate_real_ddl() -> Result<()> {
    for dialect in [Dialect::Mysql, Dialect::Postgres] {
        let mut edit = edit(dialect);
        let index = &mut edit.after.indexes[0];
        index.kind = Kind::Unique;
        index.fields.insert(
            0,
            Field {
                name: "id".into(),
                descending: true,
                ..Default::default()
            },
        );
        index.comment = "'注释' \\path".into();
        let plan = edit.plan(dialect, true)?;
        let sql = plan.statements.join(";");
        assert!(sql.contains("DROP INDEX"));
        assert!(sql.contains("UNIQUE"));
        assert!(sql.contains("DESC"));
        assert!(sql.contains("注释"));
        if matches!(dialect, Dialect::Mysql) {
            assert_eq!(plan.statements.len(), 1);
            assert!(sql.starts_with("ALTER TABLE"));
        } else {
            assert!(sql.find("DROP INDEX").unwrap() < sql.find("CREATE UNIQUE INDEX").unwrap());
        }
    }
    Ok(())
}
#[test]
fn index_guards_preserve_opaque_definitions_constraints_and_invalid_drafts() -> Result<()> {
    let mut edit = edit(Dialect::Postgres);
    edit.before.definition.indexes[0].opaque = "partial INCLUDE expression".into();
    edit.after.indexes = edit.before.definition.indexes.clone();
    edit.after.indexes[0].name = "safe_rename".into();
    assert!(edit.plan(Dialect::Postgres, false).is_ok());
    edit.after.indexes[0].method = "gin".into();
    assert!(edit.plan(Dialect::Postgres, false).is_err());
    edit.after.indexes = edit.before.definition.indexes.clone();
    edit.before.definition.indexes[0].constraint = true;
    edit.after.indexes[0].constraint = true;
    edit.after.indexes.clear();
    assert!(edit.plan(Dialect::Postgres, false).is_err());
    let mut create = definition(Dialect::Mysql);
    create.indexes[0].fields[0].prefix = Some(10);
    assert!(create.sql(Dialect::Mysql)?.contains("`label`(10)"));
    create.indexes[0].method = "BTREE;DROP TABLE x".into();
    assert!(create.sql(Dialect::Mysql).is_err());
    create.indexes[0].method = "BTREE".into();
    create.indexes[0].fields[0].name = "missing".into();
    assert!(create.sql(Dialect::Mysql).is_err());
    Ok(())
}
#[test]
fn indexes_in_create_table_are_atomic_and_identifiers_are_quoted() -> Result<()> {
    let mut create = definition(Dialect::Mysql);
    create.indexes[0].name = "x` name ON `owned`.`items`".into();
    let sql = create.statements_for_session(Dialect::Mysql, false)?;
    assert_eq!(sql.len(), 1);
    assert!(sql[0].contains("INDEX `x`` name ON ``owned``.``items```"));
    create.indexes[0].method = "btree".into();
    assert_eq!(
        create
            .statements_for_session(Dialect::Postgres, false)?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn oracle_index_methods_generate_qualified_names_without_mysql_clauses() -> Result<()> {
    let create = definition(Dialect::Oracle);
    for (method, prefix, suffix) in [
        ("BTREE", "CREATE INDEX", ""),
        ("REVERSE", "CREATE INDEX", " REVERSE"),
        ("BITMAP", "CREATE BITMAP INDEX", ""),
    ] {
        let index = Index {
            name: "ix".into(),
            method: method.into(),
            fields: vec![Field {
                name: "id".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            index.create_sql(Dialect::Oracle, &create.table, &create.columns, false)?,
            format!("{prefix} \"owned\".\"ix\" ON \"owned\".\"items\" (\"id\"){suffix}")
        );
    }
    Ok(())
}
