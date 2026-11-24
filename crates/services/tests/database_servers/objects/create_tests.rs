use super::*;
use tomato_core::table_design::{Column, Create, Options};

pub(super) async fn run(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
) -> Result<()> {
    let mut routed = connection.clone();
    if kind == "MYSQL" {
        routed.set("database", scope);
    }
    let dialect = tomato_services::dialect::resolve(kind)?;
    let DatabaseOutput::DesignMetadata(metadata) = db
        .workbench(&routed, DatabaseAction::DesignMetadata)
        .await?
    else {
        anyhow::bail!("design metadata required")
    };
    assert!(metadata.version.is_some());
    assert!(metadata.warnings.is_empty(), "{:?}", metadata.warnings);
    assert!(metadata.retained_bytes() <= 8 * 1024 * 1024);
    if kind == "MYSQL" {
        assert!(metadata.data_types.contains(&"JSON"));
        assert!(metadata.engines.iter().any(|engine| engine == "InnoDB"));
        assert!(
            metadata
                .charsets
                .iter()
                .any(|charset| charset.name == "utf8mb4" && !charset.collations.is_empty())
        );
    } else {
        assert!(metadata.data_types.contains(&"int4multirange"));
        assert!(
            metadata
                .charsets
                .iter()
                .any(|charset| charset.name == "UTF8" && !charset.collations.is_empty())
        );
    }
    let mode_sql = if kind == "MYSQL" {
        "SELECT @@SESSION.sql_mode"
    } else {
        "SHOW standard_conforming_strings"
    };
    let original_mode = db.query(&routed, mode_sql).await?[0].rows[0][0].clone();
    let set_mode = |value: &str| {
        if kind == "MYSQL" {
            format!("SET SESSION sql_mode='{}'", value.replace('\'', "''"))
        } else {
            format!("SET standard_conforming_strings={value}")
        }
    };
    let value = r"值 C:\a'\b";
    let comment = r"备注 C:\a'\b";
    let mut tables = Vec::new();
    let result: Result<()> = async {
        for off in [false, true] {
            let mode = if kind == "MYSQL" {
                let mut modes: Vec<_> = original_mode
                    .split(',')
                    .filter(|mode| {
                        !mode.eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES") && !mode.is_empty()
                    })
                    .collect();
                if off {
                    modes.push("NO_BACKSLASH_ESCAPES");
                }
                modes.join(",")
            } else {
                if off { "off" } else { "on" }.into()
            };
            db.query(&routed, &set_mode(&mode)).await?;
            let selected_mode = db.query(&routed, mode_sql).await?[0].rows[0][0].clone();
            let table = TableId {
                schema: scope.into(),
                name: format!("new`\"表_{}", u8::from(off)),
            };
            let request = Create {
                auxiliary: Vec::new(),
                indexes: Vec::new(),
                table: table.clone(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: "INTEGER".into(),
                        not_null: true,
                        primary_key: true,
                        auto_increment: kind == "MYSQL",
                        ..Default::default()
                    },
                    Column {
                        name: "name'中文".into(),
                        data_type: "VARCHAR".into(),
                        length: "64".into(),
                        default_value: value.into(),
                        comment: comment.into(),
                        ..Default::default()
                    },
                ],
                options: Options {
                    engine: if kind == "MYSQL" {
                        "InnoDB".into()
                    } else {
                        String::new()
                    },
                    ..Default::default()
                },
                comment: comment.into(),
            };
            let output = db
                .workbench(
                    &routed,
                    DatabaseAction::CreateTable {
                        request: request.clone(),
                    },
                )
                .await?;
            assert!(matches!(output,DatabaseOutput::TableCreated(created) if created==table));
            assert_eq!(
                db.query(&routed, mode_sql).await?[0].rows[0][0],
                selected_mode,
                "creation must not change SQL mode"
            );
            let qualified = format!("{}.{}", dialect.quote(scope)?, dialect.quote(&table.name)?);
            db.query(&routed, &format!("INSERT INTO {qualified} (id) VALUES(1)"))
                .await?;
            let DatabaseOutput::Page(page) = db
                .workbench(
                    &routed,
                    DatabaseAction::Page {
                        request: PageRequest {
                            table: table.clone(),
                            offset: 0,
                            limit: 10,
                            sort: None,
                            query: Default::default(),
                        },
                    },
                )
                .await?
            else {
                anyhow::bail!("page required")
            };
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0][1], Cell::Text(value.into()));
            let DatabaseOutput::Objects(catalog) = db
                .workbench(
                    &routed,
                    DatabaseAction::Objects {
                        schema: scope.into(),
                    },
                )
                .await?
            else {
                anyhow::bail!("catalog required")
            };
            let object = catalog
                .objects
                .iter()
                .find(|object| object.table == table)
                .unwrap();
            assert_eq!(object.comment, comment);
            assert_eq!(object.fields[1].comment, comment);
            tables.push((request, qualified));
        }
        db.workbench(
            &routed,
            DatabaseAction::Transaction {
                action: TransactionAction::Begin,
            },
        )
        .await?;
        db.query(
            &routed,
            &format!("INSERT INTO {} (id) VALUES(2)", tables[0].1),
        )
        .await?;
        assert!(matches!(
            db.workbench(&routed, DatabaseAction::DesignMetadata)
                .await?,
            DatabaseOutput::DesignMetadata(_)
        ));
        let mut transactional = tables[0].0.clone();
        transactional.table.name = "transaction_probe".into();
        let created = db
            .workbench(
                &routed,
                DatabaseAction::CreateTable {
                    request: transactional,
                },
            )
            .await;
        if kind == "MYSQL" {
            assert!(created.is_err());
        } else {
            assert!(matches!(created?, DatabaseOutput::TableCreated(_)));
            assert!(
                db.workbench(
                    &routed,
                    DatabaseAction::CreateTable {
                        request: tables[0].0.clone()
                    }
                )
                .await
                .is_err()
            );
            assert_eq!(
                db.query(&routed, &format!("SELECT COUNT(*) FROM {}", tables[0].1))
                    .await?[0]
                    .rows[0][0],
                "2"
            );
        }
        db.workbench(
            &routed,
            DatabaseAction::Transaction {
                action: TransactionAction::Rollback,
            },
        )
        .await?;
        assert_eq!(
            db.query(&routed, &format!("SELECT COUNT(*) FROM {}", tables[0].1))
                .await?[0]
                .rows[0][0],
            "1"
        );
        let DatabaseOutput::Objects(catalog) = db
            .workbench(
                &routed,
                DatabaseAction::Objects {
                    schema: scope.into(),
                },
            )
            .await?
        else {
            anyhow::bail!("catalog required")
        };
        assert!(
            !catalog
                .objects
                .iter()
                .any(|object| object.table.name == "transaction_probe")
        );
        let mut invalid = tables[0].0.clone();
        invalid.table.name = "invalid_probe".into();
        invalid.columns[0].data_type.clear();
        assert!(
            db.workbench(&routed, DatabaseAction::CreateTable { request: invalid })
                .await
                .is_err()
        );
        let DatabaseOutput::Objects(catalog) = db
            .workbench(
                &routed,
                DatabaseAction::Objects {
                    schema: scope.into(),
                },
            )
            .await?
        else {
            anyhow::bail!("catalog required")
        };
        assert!(
            !catalog
                .objects
                .iter()
                .any(|object| object.table.name == "invalid_probe")
        );
        Ok(())
    }
    .await;
    let rollback = db
        .workbench(
            &routed,
            DatabaseAction::Transaction {
                action: TransactionAction::Rollback,
            },
        )
        .await;
    let restore = db.query(&routed, &set_mode(&original_mode)).await;
    result?;
    rollback?;
    restore?;
    Ok(())
}
