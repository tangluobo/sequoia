//! Lightweight editor snapshots retain guards; complete SQL stays independently usable.
use super::*;
use tomato_core::table_design::{Column, alter::Edit};

pub(super) async fn run(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
) -> Result<()> {
    let dialect = tomato_services::dialect::resolve(kind)?;
    let q = |s: &str| dialect.quote(s);
    let table = TableId {
        schema: scope.into(),
        name: "lazy_items".into(),
    };
    let target = format!("{}.{}", q(scope)?, q(&table.name)?);
    db.query(
        connection,
        &format!("CREATE TABLE {target} (id INTEGER,label VARCHAR(20))"),
    )
    .await?;
    let DatabaseOutput::Design(light) = db
        .workbench(
            connection,
            DatabaseAction::DesignStructure {
                table: table.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("structure required")
    };
    assert!(light.sql.is_empty());
    let DatabaseOutput::Design(full) = db
        .workbench(
            connection,
            DatabaseAction::Design {
                table: table.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("full definition required")
    };
    assert!(!full.sql.is_empty());
    assert_eq!(light.definition, full.definition);
    assert_eq!(light.raw_columns, full.raw_columns);
    let DatabaseOutput::DesignSql {
        table: returned,
        sql,
    } = db
        .workbench(
            connection,
            DatabaseAction::DesignSql {
                table: table.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("SQL required")
    };
    assert_eq!(returned, table);
    assert_eq!(sql, full.sql);
    let mut after = light.definition.clone();
    after.columns.push(Column {
        name: "amount".into(),
        data_type: "INTEGER".into(),
        ..Default::default()
    });
    let edit = Edit {
        before: light.baseline(),
        after,
        origins: vec![Some("id".into()), Some("label".into()), None],
    };
    let DatabaseOutput::Design(saved) = db
        .workbench(
            connection,
            DatabaseAction::SaveStructure {
                edit: Box::new(edit.clone()),
            },
        )
        .await?
    else {
        anyhow::bail!("saved structure required")
    };
    assert!(saved.sql.is_empty());
    assert_eq!(saved.definition.columns.len(), 3);
    assert!(
        db.workbench(
            connection,
            DatabaseAction::SaveStructure {
                edit: Box::new(edit)
            }
        )
        .await
        .is_err(),
        "a lightweight snapshot must still reject stale saves"
    );
    let DatabaseOutput::DesignSql { sql, .. } = db
        .workbench(
            connection,
            DatabaseAction::DesignSql {
                table: table.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("saved SQL required")
    };
    assert!(sql.contains("amount"));
    let copy = format!("{}.{}", q(scope)?, q("lazy_copy")?);
    // SHOW CREATE TABLE uses an unqualified MySQL name; retain that canonical
    // preview while directing the copied statement into this owned schema.
    let original = if sql.contains(&target) {
        target.clone()
    } else {
        q(&table.name)?
    };
    assert!(sql.contains(&original));
    db.query(connection, &sql.replace(&original, &copy)).await?;
    db.query(
        connection,
        &format!("INSERT INTO {copy} VALUES(1,'copied',7)"),
    )
    .await?;
    let values = db
        .query(connection, &format!("SELECT amount FROM {copy}"))
        .await?;
    assert_eq!(values[0].rows[0][0], "7");
    let mut after = saved.definition.clone();
    after.columns[1].comment = "legacy full save".into();
    let edit = Edit {
        before: saved.baseline(),
        after,
        origins: saved
            .definition
            .columns
            .iter()
            .map(|c| Some(c.name.clone()))
            .collect(),
    };
    let DatabaseOutput::Design(legacy_saved) = db
        .workbench(
            connection,
            DatabaseAction::SaveDesign {
                edit: Box::new(edit),
            },
        )
        .await?
    else {
        anyhow::bail!("legacy save output required")
    };
    assert!(!legacy_saved.sql.is_empty());
    assert_eq!(
        legacy_saved.definition.columns[1].comment,
        "legacy full save"
    );
    let view = TableId {
        schema: scope.into(),
        name: "lazy_view".into(),
    };
    let qualified_view = format!("{}.{}", q(scope)?, q(&view.name)?);
    db.query(
        connection,
        &format!("CREATE VIEW {qualified_view} AS SELECT id FROM {target}"),
    )
    .await?;
    let DatabaseOutput::Design(view_structure) = db
        .workbench(
            connection,
            DatabaseAction::DesignStructure {
                table: view.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("view structure required")
    };
    assert!(view_structure.view && view_structure.sql.is_empty());
    let DatabaseOutput::DesignSql { sql, .. } = db
        .workbench(
            connection,
            DatabaseAction::DesignSql {
                table: view.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("view SQL required")
    };
    assert!(sql.contains("VIEW") && sql.contains("id"));
    if kind == "MYSQL" {
        mysql_view_permissions(db, connection, scope, &view).await?;
    }
    Ok(())
}
async fn mysql_view_permissions(
    db: &mut Databases,
    root: &Connection,
    scope: &str,
    view: &TableId,
) -> Result<()> {
    let name = format!(
        "tomato_lazy_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let account = format!("'{name}'@'%'");
    let target = format!("`{scope}`.`{}`", view.name);
    db.query(
        root,
        &format!("CREATE USER {account} IDENTIFIED BY 'OwnedFixture42'"),
    )
    .await?;
    let mut reader = root.clone();
    reader.id = Connection::new("MYSQL").id;
    reader.set("username", name.as_str());
    reader.set("password", "OwnedFixture42");
    reader.set("database", scope);
    let result: Result<()> = async {
        db.query(root, &format!("GRANT SELECT ON {target} TO {account}"))
            .await?;
        let DatabaseOutput::Design(snapshot) = db
            .workbench(
                &reader,
                DatabaseAction::DesignStructure {
                    table: view.clone(),
                },
            )
            .await?
        else {
            anyhow::bail!("limited view snapshot required")
        };
        anyhow::ensure!(
            snapshot.view && snapshot.sql.is_empty() && snapshot.definition.columns.len() == 1,
            "SELECT-only view structure did not load"
        );
        anyhow::ensure!(
            db.workbench(
                &reader,
                DatabaseAction::DesignSql {
                    table: view.clone()
                }
            )
            .await
            .is_err(),
            "SHOW VIEW privilege must still be required for original SQL"
        );
        db.query(root, &format!("GRANT SHOW VIEW ON {target} TO {account}"))
            .await?;
        db.disconnect(&reader.id).await?;
        let DatabaseOutput::DesignSql { sql, .. } = db
            .workbench(
                &reader,
                DatabaseAction::DesignSql {
                    table: view.clone(),
                },
            )
            .await?
        else {
            anyhow::bail!("view SQL after grant required")
        };
        anyhow::ensure!(
            sql.contains("VIEW"),
            "view definition not returned after privilege grant"
        );
        Ok(())
    }
    .await;
    let close = db.disconnect(&reader.id).await;
    let cleanup = db
        .query(root, &format!("DROP USER {account}"))
        .await
        .map(|_| ());
    result.and(close).and(cleanup)
}
