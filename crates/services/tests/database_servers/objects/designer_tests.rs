use super::*;
use tomato_core::table_design::{
    Column,
    alter::{Edit, Snapshot},
};
pub(super) async fn load(db: &mut Databases, c: &Connection, table: &TableId) -> Result<Snapshot> {
    let DatabaseOutput::Design(snapshot) = db
        .workbench(
            c,
            DatabaseAction::Design {
                table: table.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("designer snapshot required")
    };
    let mut snapshot = *snapshot;
    use tomato_core::table_design::alter::InfoPage;
    for page in [InfoPage::Indexes, InfoPage::ForeignKeys, InfoPage::Triggers] {
        let DatabaseOutput::DesignInfo { info, .. } = db
            .workbench(
                c,
                DatabaseAction::DesignInfo {
                    table: table.clone(),
                    page,
                },
            )
            .await?
        else {
            anyhow::bail!("designer information page required")
        };
        match page {
            InfoPage::Indexes => snapshot.indexes = info,
            InfoPage::ForeignKeys => snapshot.foreign_keys = info,
            InfoPage::Triggers => snapshot.triggers = info,
        }
    }
    Ok(snapshot)
}
pub(super) fn edit(snapshot: Snapshot) -> Edit {
    let origins = snapshot
        .definition
        .columns
        .iter()
        .map(|c| Some(c.name.clone()))
        .collect();
    Edit {
        after: snapshot.definition.clone(),
        before: snapshot.into(),
        origins,
    }
}
pub(super) async fn save(db: &mut Databases, c: &Connection, edit: Edit) -> Result<Snapshot> {
    let DatabaseOutput::Design(snapshot) = db
        .workbench(
            c,
            DatabaseAction::SaveDesign {
                edit: Box::new(edit),
            },
        )
        .await?
    else {
        anyhow::bail!("saved snapshot required")
    };
    Ok(*snapshot)
}
pub(super) async fn run(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
) -> Result<()> {
    let mut c = connection.clone();
    if kind == "MYSQL" {
        c.set("database", scope);
    }
    let dialect = tomato_services::dialect::resolve(kind)?;
    let table = TableId {
        schema: scope.into(),
        name: "designer_表".into(),
    };
    let qualified = format!("{}.{}", dialect.quote(scope)?, dialect.quote(&table.name)?);
    let ddl = if kind == "MYSQL" {
        format!(
            "CREATE TABLE {qualified} (id INTEGER NOT NULL AUTO_INCREMENT PRIMARY KEY,amount INTEGER DEFAULT 3,value VARCHAR(30) DEFAULT 'old' COMMENT '备注',stamp TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),generated_value INTEGER GENERATED ALWAYS AS (amount+1) STORED,KEY ix_value(value)) ENGINE=InnoDB COMMENT='表备注'"
        )
    } else {
        format!(
            "CREATE TABLE {qualified} (id INTEGER PRIMARY KEY,amount INTEGER DEFAULT 3,value VARCHAR(30) DEFAULT 'old',stamp TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP,generated_value INTEGER GENERATED ALWAYS AS (amount+1) STORED)"
        )
    };
    db.query(&c, &ddl).await?;
    if kind == "POSTGRESQL" {
        db.query(&c,&format!("CREATE INDEX ix_designer ON {qualified}(value); COMMENT ON COLUMN {qualified}.value IS '备注'; COMMENT ON TABLE {qualified} IS '表备注'")).await?;
    }
    db.query(
        &c,
        &format!("INSERT INTO {qualified}(id,amount) VALUES(1,7)"),
    )
    .await?;
    let initial = load(db, &c, &table).await?;
    assert_eq!(initial.definition.columns.len(), 5);
    assert_eq!(initial.definition.comment, "表备注");
    assert!(initial.indexes.rows.iter().any(|r| r[0].starts_with("ix_")));
    let original_stamp = initial.raw_columns.get(3).cloned();
    let original_generated = initial.raw_columns.get(4).cloned();
    let mut changes = edit(initial);
    changes.after.columns[2].comment = "新的 '备注' C:\\path".into();
    changes.after.comment = "表的新备注".into();
    let mut saved = save(db, &c, changes).await?;
    assert_eq!(saved.definition.columns[2].comment, "新的 '备注' C:\\path");
    assert_eq!(saved.definition.comment, "表的新备注");
    if kind == "MYSQL" {
        assert_eq!(saved.raw_columns.get(3), original_stamp.as_ref());
        assert_eq!(saved.raw_columns.get(4), original_generated.as_ref());
    }
    let mut changes = edit(saved);
    changes.after.columns[2].name = "display_值".into();
    if kind == "MYSQL" {
        let col = changes.after.columns.remove(2);
        changes.after.columns.insert(0, col);
        let origin = changes.origins.remove(2);
        changes.origins.insert(0, origin);
    }
    changes.after.columns.push(Column {
        name: "added".into(),
        data_type: "INTEGER".into(),
        default_value: "9".into(),
        ..Default::default()
    });
    changes.origins.push(None);
    saved = save(db, &c, changes).await?;
    if kind == "MYSQL" {
        assert_eq!(saved.definition.columns[0].name, "display_值");
    }
    let query = format!(
        "SELECT {},added,generated_value FROM {qualified} WHERE id=1",
        dialect.quote("display_值")?
    );
    let data = db.query(&c, &query).await?;
    assert_eq!(data[0].rows[0], ["old", "9", "8"]);
    let mut changes = edit(saved);
    let index = changes
        .after
        .columns
        .iter()
        .position(|c| c.name == "added")
        .unwrap();
    changes.after.columns.remove(index);
    changes.origins.remove(index);
    saved = save(db, &c, changes).await?;
    assert!(!saved.definition.columns.iter().any(|c| c.name == "added"));
    let mut too_short = edit(saved.clone());
    let at = too_short
        .after
        .columns
        .iter()
        .position(|column| column.name == "display_值")
        .unwrap();
    too_short.after.columns[at].length = "1".into();
    assert!(
        save(db, &c, too_short).await.is_err(),
        "shortening must reject data loss instead of pre-truncating the USING expression"
    );
    assert_eq!(
        db.query(
            &c,
            &format!(
                "SELECT {} FROM {qualified} WHERE id=1",
                dialect.quote("display_值")?
            )
        )
        .await?[0]
            .rows[0][0],
        "old"
    );
    let stale = edit(saved);
    db.query(
        &c,
        &format!("ALTER TABLE {qualified} ADD COLUMN external_change INTEGER"),
    )
    .await?;
    assert!(
        db.workbench(
            &c,
            DatabaseAction::SaveDesign {
                edit: Box::new(stale)
            }
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("其他操作")
    );
    let snapshot = load(db, &c, &table).await?;
    db.workbench(
        &c,
        DatabaseAction::Transaction {
            action: TransactionAction::Begin,
        },
    )
    .await?;
    let outcome: Result<()> = async {
        db.query(
            &c,
            &format!("INSERT INTO {qualified}(id,amount) VALUES(88,88)"),
        )
        .await?;
        let mut changes = edit(snapshot);
        let index = changes
            .after
            .columns
            .iter()
            .position(|c| c.name == "display_值")
            .unwrap();
        if kind == "MYSQL" {
            changes.after.columns[index].comment = "must refuse implicit commit".into();
        } else {
            changes.after.columns[index].data_type = "INTEGER".into();
            changes.after.columns[index].length.clear();
            changes.after.columns[index].scale.clear();
        }
        let error = db
            .workbench(
                &c,
                DatabaseAction::SaveDesign {
                    edit: Box::new(changes),
                },
            )
            .await
            .unwrap_err();
        if kind == "MYSQL" {
            assert!(error.to_string().contains("未完成事务"));
        }
        assert_eq!(
            db.query(&c, &format!("SELECT amount FROM {qualified} WHERE id=88"))
                .await?[0]
                .rows[0][0],
            "88"
        );
        Ok(())
    }
    .await;
    db.workbench(
        &c,
        DatabaseAction::Transaction {
            action: TransactionAction::Rollback,
        },
    )
    .await?;
    outcome?;
    if kind == "POSTGRESQL" {
        let extra = TableId {
            schema: scope.into(),
            name: "designer_precision".into(),
        };
        let extra_name = format!("{}.{}", dialect.quote(scope)?, dialect.quote(&extra.name)?);
        db.query(&c,&format!("CREATE TABLE {extra_name}(id INTEGER PRIMARY KEY,coarse NUMERIC(2,-3),stamp TIMESTAMPTZ(4));INSERT INTO {extra_name}(id,coarse) VALUES(1,12000)")).await?;
        let state = load(db, &c, &extra).await?;
        assert_eq!(state.definition.columns[1].scale, "-3");
        assert_eq!(state.definition.columns[2].data_type, "timestamptz");
        assert_eq!(state.definition.columns[2].scale, "4");
        let mut request = edit(state);
        request.after.columns[1].comment = "negative scale remains".into();
        request.after.columns[2].scale = "2".into();
        let state = save(db, &c, request).await?;
        assert_eq!(state.definition.columns[1].scale, "-3");
        assert_eq!(state.definition.columns[2].scale, "2");
        assert_eq!(
            db.query(&c, &format!("SELECT coarse FROM {extra_name} WHERE id=1"))
                .await?[0]
                .rows[0][0],
            "12000"
        );
    }
    assert_eq!(
        db.query(&c, &format!("SELECT COUNT(*) FROM {qualified} WHERE id=88"))
            .await?[0]
            .rows[0][0],
        "0"
    );
    Ok(())
}
