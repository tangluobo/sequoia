//! Native CREATE/ALTER and metadata read-back; assertions exercise real constraints and triggers.
use super::designer_tests::{edit, load, save};
use super::*;
use tomato_core::table_design::{Column, Create, Options, alter::InfoPage, auxiliary};
pub(super) async fn run(db: &mut Databases, c: &Connection, kind: &str, scope: &str) -> Result<()> {
    let dialect = tomato_services::dialect::resolve(kind)?;
    let q = |name: &str| dialect.quote(name);
    let table = TableId {
        schema: scope.into(),
        name: "aux_child".into(),
    };
    let child = format!("{}.{}", q(scope)?, q(&table.name)?);
    let parent = format!("{}.{}", q(scope)?, q("aux_parent")?);
    db.query(c,&format!("CREATE TABLE {parent} (a INTEGER NOT NULL,b INTEGER NOT NULL,PRIMARY KEY(a,b)); INSERT INTO {parent} VALUES(1,2)")).await?;
    let function = format!("{}.{}", q(scope)?, q("aux_fill")?);
    if kind == "POSTGRESQL" {
        db.query(c,&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.note := 'triggered'; RETURN NEW; END $$")).await?;
    }
    let fk = vec![
        "fk_aux".into(),
        "a,b".into(),
        scope.into(),
        "aux_parent".into(),
        "a,b".into(),
        "CASCADE".into(),
        "NO ACTION".into(),
    ];
    let trigger = vec![
        "tr_aux".into(),
        "BEFORE".into(),
        "INSERT".into(),
        if kind == "MYSQL" {
            "SET NEW.note = 'triggered'".into()
        } else {
            format!("{function}()")
        },
    ];
    let request = Create {
        auxiliary: vec![
            auxiliary::Edit {
                before_signature: String::new(),
                page: InfoPage::ForeignKeys,
                before: vec![],
                after: vec![fk],
            },
            auxiliary::Edit {
                before_signature: String::new(),
                page: InfoPage::Triggers,
                before: vec![],
                after: vec![trigger],
            },
        ],
        indexes: vec![],
        table: table.clone(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: "INTEGER".into(),
                not_null: true,
                primary_key: true,
                auto_increment: true,
                ..Default::default()
            },
            Column {
                name: "a".into(),
                data_type: "INTEGER".into(),
                ..Default::default()
            },
            Column {
                name: "b".into(),
                data_type: "INTEGER".into(),
                ..Default::default()
            },
            Column {
                name: "note".into(),
                data_type: "VARCHAR".into(),
                length: "100".into(),
                ..Default::default()
            },
        ],
        options: if kind == "MYSQL" {
            Options {
                engine: "InnoDB".into(),
                charset: "utf8mb4".into(),
                collation: "utf8mb4_unicode_ci".into(),
                auto_increment: "100".into(),
                row_format: "Compact".into(),
                avg_row_length: "256".into(),
            }
        } else {
            Options::default()
        },
        comment: "aux baseline".into(),
    };
    let sql = request.sql(tomato_core::table_design::Dialect::resolve(kind)?)?;
    assert!(sql.contains("ADD CONSTRAINT") && sql.contains("CREATE TRIGGER"));
    db.workbench(c, DatabaseAction::CreateTable { request })
        .await?;
    db.query(
        c,
        &format!("INSERT INTO {child}(a,b,note) VALUES(1,2,'old')"),
    )
    .await?;
    assert!(
        db.query(c, &format!("INSERT INTO {child}(a,b) VALUES(9,9)"))
            .await
            .is_err()
    );
    let data = db.query(c, &format!("SELECT id,note FROM {child}")).await?;
    assert_eq!(data[0].rows[0][1], "triggered");
    let mut snapshot = load(db, c, &table).await?;
    assert_eq!(snapshot.foreign_keys.rows.len(), 1);
    assert_eq!(snapshot.triggers.rows.len(), 1);
    if kind == "MYSQL" {
        assert_eq!(data[0].rows[0][0], "100");
        assert_eq!(snapshot.definition.options.avg_row_length, "256");
        let mut options = edit(snapshot);
        options.after.options.row_format = "Dynamic".into();
        options.after.options.avg_row_length = "512".into();
        options.after.options.auto_increment = "500".into();
        let plan = options.plan(tomato_core::table_design::Dialect::Mysql, true)?;
        assert_eq!(plan.statements.len(), 1);
        assert!(
            plan.statements[0].contains("ROW_FORMAT=Dynamic")
                && plan.statements[0].contains("AVG_ROW_LENGTH=512")
        );
        snapshot = save(db, c, options).await?;
        assert_eq!(snapshot.definition.options.row_format, "Dynamic");
        assert_eq!(snapshot.definition.options.avg_row_length, "512");
        // An unrelated data insert must not be mistaken for a schema conflict.
        db.query(c, &format!("INSERT INTO {child}(a,b) VALUES(1,2)"))
            .await?;
        let mut comment = edit(snapshot.clone());
        comment.after.comment = "counter advanced".into();
        snapshot = save(db, c, comment).await?;
        assert_eq!(snapshot.definition.comment, "counter advanced");
        let stale = snapshot.clone();
        db.query(c, &format!("INSERT INTO {child}(a,b) VALUES(1,2)"))
            .await?;
        let mut counter = edit(stale);
        counter.after.options.auto_increment = "900".into();
        assert!(
            save(db, c, counter).await.is_err(),
            "explicit counter changes need the original counter to remain current"
        );
        snapshot = load(db, c, &table).await?;
    }
    if kind == "POSTGRESQL" {
        let mut stale = edit(snapshot.clone());
        stale.after.auxiliary = vec![auxiliary::Edit {
            page: InfoPage::ForeignKeys,
            before_signature: snapshot.foreign_keys.signature.clone(),
            before: snapshot.foreign_keys.rows.clone(),
            after: vec![],
        }];
        stale.after.comment = "hidden change must stop save".into();
        db.query(
            c,
            &format!("ALTER TABLE {child} ALTER CONSTRAINT fk_aux DEFERRABLE INITIALLY DEFERRED"),
        )
        .await?;
        let error = save(db, c, stale).await.unwrap_err().to_string();
        assert!(error.contains("外键或触发器"), "{error}");
        snapshot = load(db, c, &table).await?;
        assert_ne!(snapshot.definition.comment, "hidden change must stop save");
    }
    let mut changes = edit(snapshot.clone());
    changes.after.auxiliary = vec![
        auxiliary::Edit {
            before_signature: snapshot.foreign_keys.signature.clone(),
            page: InfoPage::ForeignKeys,
            before: snapshot.foreign_keys.rows.clone(),
            after: vec![],
        },
        auxiliary::Edit {
            before_signature: snapshot.triggers.signature.clone(),
            page: InfoPage::Triggers,
            before: snapshot.triggers.rows.clone(),
            after: vec![],
        },
    ];
    let preview = changes.plan(
        tomato_core::table_design::Dialect::resolve(kind)?,
        kind == "MYSQL",
    )?;
    assert_eq!(preview.statements.len(), 2);
    let saved = save(db, c, changes).await?;
    assert!(saved.foreign_keys.rows.is_empty() && saved.triggers.rows.is_empty());
    db.query(
        c,
        &format!("INSERT INTO {child}(a,b,note) VALUES(9,9,'unmodified')"),
    )
    .await?;
    let values = db
        .query(c, &format!("SELECT note FROM {child} WHERE a=9"))
        .await?;
    assert_eq!(values[0].rows[0][0], "unmodified");
    let mut saved = saved;
    if kind == "MYSQL" {
        let mut engine = edit(saved);
        engine.after.options.engine = "MyISAM".into();
        saved = save(db, c, engine).await?;
        assert_eq!(saved.definition.options.engine, "MyISAM");
        let mut encoding = edit(saved);
        encoding.after.options.engine = "InnoDB".into();
        encoding.after.options.charset = "latin1".into();
        encoding.after.options.collation = "latin1_general_ci".into();
        saved = save(db, c, encoding).await?;
        assert_eq!(saved.definition.options.engine, "InnoDB");
        assert_eq!(saved.definition.options.charset, "latin1");
        assert_eq!(saved.definition.options.collation, "latin1_general_ci");
    }
    // A concurrent metadata change must stop the whole save before its first DDL.
    let mut stale = edit(saved);
    stale.after.comment = "must not apply".into();
    stale.after.auxiliary = vec![auxiliary::Edit {
        before_signature: String::new(),
        page: InfoPage::Triggers,
        before: vec![],
        after: vec![vec![
            "tr_new".into(),
            "BEFORE".into(),
            "INSERT".into(),
            if kind == "MYSQL" {
                "SET NEW.note='new'".into()
            } else {
                format!("{function}()")
            },
        ]],
    }];
    let external = if kind == "MYSQL" {
        format!(
            "CREATE TRIGGER {}.{} BEFORE INSERT ON {child} FOR EACH ROW SET NEW.note='external'",
            q(scope)?,
            q("tr_external")?
        )
    } else {
        format!(
            "CREATE TRIGGER {} BEFORE INSERT ON {child} FOR EACH ROW EXECUTE FUNCTION {function}()",
            q("tr_external")?
        )
    };
    db.query(c, &external).await?;
    let error = save(db, c, stale).await.unwrap_err().to_string();
    assert!(error.contains("外键或触发器"), "{error}");
    assert_ne!(
        load(db, c, &table).await?.definition.comment,
        "must not apply"
    );
    Ok(())
}
