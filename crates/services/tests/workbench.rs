use anyhow::Result;
use tomato_core::{Connection, workbench::*};
use tomato_services::database::{Databases, inserts_sql};
#[path = "table_filters/mod.rs"]
mod table_filters;

#[tokio::test]
async fn table_filter_sort_count_paging_and_export_execute_on_the_database() -> Result<()> {
    let profile = tempfile::tempdir()?;
    let mut connection = Connection::new("SQLITE");
    connection.set(
        "database",
        profile.path().join("filter.db").to_string_lossy().as_ref(),
    );
    table_filters::verify(&mut Databases::default(), &connection, "main").await
}

fn request(name: &str) -> PageRequest {
    PageRequest {
        table: TableId {
            schema: "main".into(),
            name: name.into(),
        },
        offset: 0,
        limit: 1000,
        sort: None,
        query: Default::default(),
    }
}

#[tokio::test]
async fn completion_uses_live_schema_views_generated_columns_and_quoted_attached_databases()
-> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c,"CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT, derived TEXT GENERATED ALWAYS AS (name || '!') VIRTUAL); CREATE VIEW names AS SELECT name FROM users; ATTACH DATABASE ':memory:' AS \"奇'库\"; CREATE TABLE \"奇'库\".isolated(other TEXT);").await?;
    let DatabaseOutput::Completion(meta) = db
        .workbench(
            &c,
            DatabaseAction::Completion {
                schema: "main".into(),
            },
        )
        .await?
    else {
        panic!("completion")
    };
    assert_eq!(meta.schema, "main");
    let users = meta
        .tables
        .iter()
        .find(|table| table.name == "users")
        .unwrap();
    assert_eq!(users.columns, ["id", "name", "derived"]);
    assert_eq!(
        meta.tables
            .iter()
            .find(|table| table.name == "names")
            .unwrap()
            .columns,
        ["name"]
    );
    assert!(!meta.tables.iter().any(|table| table.name == "isolated"));
    let result = tomato_core::sql_completion::complete("users.d", 7, &meta);
    assert_eq!(result.candidates[0].text, "derived");
    let DatabaseOutput::Completion(other) = db
        .workbench(
            &c,
            DatabaseAction::Completion {
                schema: "奇'库".into(),
            },
        )
        .await?
    else {
        panic!("completion")
    };
    assert_eq!(other.tables.len(), 1);
    assert_eq!(other.tables[0].name, "isolated");
    assert_eq!(other.tables[0].columns, ["other"]);
    db.query(&c, "ALTER TABLE users ADD COLUMN added TEXT")
        .await?;
    let DatabaseOutput::Completion(updated) = db
        .workbench(
            &c,
            DatabaseAction::Completion {
                schema: "main".into(),
            },
        )
        .await?
    else {
        panic!("completion")
    };
    assert!(
        updated
            .tables
            .iter()
            .find(|table| table.name == "users")
            .unwrap()
            .columns
            .iter()
            .any(|name| name == "added")
    );
    db.disconnect(&c.id).await?;
    Ok(())
}
async fn load(db: &mut Databases, c: &Connection, request: PageRequest) -> Result<TablePage> {
    match db.workbench(c, DatabaseAction::Page { request }).await? {
        DatabaseOutput::Page(page) => Ok(page),
        _ => unreachable!(),
    }
}
async fn save(
    db: &mut Databases,
    c: &Connection,
    name: &str,
    changes: Vec<RowChange>,
) -> Result<TablePage> {
    let request = request(name);
    match db
        .workbench(
            c,
            DatabaseAction::Apply {
                table: request.table.clone(),
                changes,
                request,
            },
        )
        .await?
    {
        DatabaseOutput::Applied {
            page: Some(page),
            refresh_error: None,
            ..
        } => Ok(page),
        output => anyhow::bail!("unexpected save result: {output:?}"),
    }
}

#[tokio::test]
async fn catalog_paging_defaults_generated_binary_and_copy_roundtrip() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "CREATE TABLE \"odd' table\" (id INTEGER PRIMARY KEY, value TEXT DEFAULT 'fallback', payload BLOB, derived TEXT GENERATED ALWAYS AS (value || '!') STORED); CREATE VIEW summary AS SELECT id FROM \"odd' table\";").await?;
    let DatabaseOutput::Databases(databases) = db.workbench(&c, DatabaseAction::Databases).await?
    else {
        panic!("databases")
    };
    assert_eq!(databases, ["main"]);
    let DatabaseOutput::Schemas(schemas) = db.workbench(&c, DatabaseAction::Schemas).await? else {
        panic!("schemas")
    };
    assert_eq!(schemas, ["main"]);
    let DatabaseOutput::Tables(tables) = db
        .workbench(
            &c,
            DatabaseAction::Tables {
                schema: "main".into(),
            },
        )
        .await?
    else {
        panic!("tables")
    };
    assert!(tables.iter().any(|t| t.table.name == "summary" && t.view));
    let page = save(
        &mut db,
        &c,
        "odd' table",
        vec![
            RowChange::Insert {
                values: vec![
                    None,
                    Some(Cell::Text("NULL".into())),
                    Some(Cell::Binary(vec![0, 255, 39])),
                    None,
                ],
            },
            RowChange::Insert {
                values: vec![None, Some(Cell::Null), None, None],
            },
            RowChange::Insert {
                values: vec![None, None, None, None],
            },
        ],
    )
    .await?;
    assert!(page.definition.columns[0].auto_increment);
    assert!(page.definition.columns[3].generated);
    assert_eq!(
        page.rows[0],
        vec![
            Cell::Integer(1),
            Cell::Text("NULL".into()),
            Cell::Binary(vec![0, 255, 39]),
            Cell::Text("NULL!".into())
        ]
    );
    assert_eq!(page.rows[1][1], Cell::Null);
    assert_eq!(page.rows[2][1], Cell::Text("fallback".into()));
    let mut r = request("odd' table");
    r.offset = 1;
    r.limit = 1;
    r.sort = Some(Sort {
        column: "id".into(),
        descending: true,
    });
    let second = load(&mut db, &c, r).await?;
    assert_eq!(second.total, 3);
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0][0], Cell::Integer(2));
    let exported = inserts_sql("SQLITE", &page.definition, &page.rows)?;
    db.query(&c, "DELETE FROM \"odd' table\"").await?;
    db.query(&c, &exported).await?;
    assert_eq!(
        load(&mut db, &c, request("odd' table")).await?.rows,
        page.rows
    );
    let view = load(&mut db, &c, request("summary")).await?;
    assert!(view.definition.view);
    assert!(
        save(
            &mut db,
            &c,
            "summary",
            vec![RowChange::Delete {
                original: view.rows[0].clone()
            }]
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn stale_row_rolls_back_entire_batch_and_generated_columns_reject_writes() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "CREATE TABLE sample (id INTEGER PRIMARY KEY, value TEXT, derived TEXT GENERATED ALWAYS AS (value || '!')); INSERT INTO sample(id,value) VALUES (1,'before'),(2,'second');").await?;
    let before = load(&mut db, &c, request("sample")).await?;
    db.query(&c, "UPDATE sample SET value='external' WHERE id=1")
        .await?;
    let mut changed = before.rows[0].clone();
    changed[1] = Cell::Text("mine".into());
    assert!(
        save(
            &mut db,
            &c,
            "sample",
            vec![
                RowChange::Delete {
                    original: before.rows[1].clone()
                },
                RowChange::Update {
                    original: before.rows[0].clone(),
                    values: changed
                },
            ]
        )
        .await
        .is_err()
    );
    let after = load(&mut db, &c, request("sample")).await?;
    assert_eq!(after.rows.len(), 2, "earlier deletion must roll back");
    assert_eq!(after.rows[0][1], Cell::Text("external".into()));
    let mut invalid = after.rows[0].clone();
    invalid[2] = Cell::Text("forged".into());
    assert!(
        save(
            &mut db,
            &c,
            "sample",
            vec![RowChange::Update {
                original: after.rows[0].clone(),
                values: invalid
            }]
        )
        .await
        .is_err()
    );
    // A constraint failure on the second insertion must also cancel the first.
    assert!(
        save(
            &mut db,
            &c,
            "sample",
            vec![
                RowChange::Insert {
                    values: vec![Some(Cell::Integer(3)), None, None]
                },
                RowChange::Insert {
                    values: vec![Some(Cell::Integer(1)), None, None]
                },
            ]
        )
        .await
        .is_err()
    );
    assert_eq!(load(&mut db, &c, request("sample")).await?.total, 2);
    Ok(())
}

#[tokio::test]
async fn duplicate_rows_update_one_physical_row_and_no_pk_delete_is_rejected() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "CREATE TABLE dup (value TEXT, optional TEXT); INSERT INTO dup VALUES ('same',NULL),('same',NULL);").await?;
    let before = load(&mut db, &c, request("dup")).await?;
    let changed = vec![Cell::Text("different".into()), Cell::Text("NULL".into())];
    let after = save(
        &mut db,
        &c,
        "dup",
        vec![RowChange::Update {
            original: before.rows[0].clone(),
            values: changed.clone(),
        }],
    )
    .await?;
    assert_eq!(after.rows.iter().filter(|r| **r == changed).count(), 1);
    assert_eq!(
        after.rows.iter().filter(|r| **r == before.rows[0]).count(),
        1
    );
    assert!(
        save(
            &mut db,
            &c,
            "dup",
            vec![RowChange::Delete {
                original: after.rows[0].clone()
            }]
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn save_preserves_raw_sql_transaction_and_failed_batch_keeps_outer_work() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "CREATE TABLE sample (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO sample VALUES (1,'original'); BEGIN; UPDATE sample SET value='outer' WHERE id=1;").await?;
    let before = load(&mut db, &c, request("sample")).await?;
    save(
        &mut db,
        &c,
        "sample",
        vec![RowChange::Update {
            original: before.rows[0].clone(),
            values: vec![Cell::Integer(1), Cell::Text("inner".into())],
        }],
    )
    .await?;
    db.workbench(
        &c,
        DatabaseAction::Transaction {
            action: TransactionAction::Rollback,
        },
    )
    .await?;
    assert_eq!(
        load(&mut db, &c, request("sample")).await?.rows[0][1],
        Cell::Text("original".into())
    );
    db.workbench(
        &c,
        DatabaseAction::Transaction {
            action: TransactionAction::Begin,
        },
    )
    .await?;
    db.query(&c, "INSERT INTO sample VALUES (2,'outer work')")
        .await?;
    assert!(
        save(
            &mut db,
            &c,
            "sample",
            vec![
                RowChange::Insert {
                    values: vec![Some(Cell::Integer(3)), None]
                },
                RowChange::Insert {
                    values: vec![Some(Cell::Integer(1)), None]
                },
            ]
        )
        .await
        .is_err()
    );
    db.workbench(
        &c,
        DatabaseAction::Transaction {
            action: TransactionAction::Commit,
        },
    )
    .await?;
    let after = load(&mut db, &c, request("sample")).await?;
    assert_eq!(after.total, 2);
    assert_eq!(after.rows[1][1], Cell::Text("outer work".into()));
    Ok(())
}

#[tokio::test]
async fn tsv_import_stages_without_writing_and_rejects_excess_rows() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "CREATE TABLE sample(id INTEGER PRIMARY KEY, value TEXT, payload BLOB, derived TEXT GENERATED ALWAYS AS (value || '!'))").await?;
    let page = load(&mut db, &c, request("sample")).await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("import.tsv");
    std::fs::write(&path, "\u{feff}\tNULL\t00ff\r\n\t中文\t\r\n")?;
    assert!(tomato_services::files::import_table_rows(&path, &page.definition, 1).is_err());
    let rows = tomato_services::files::import_table_rows(&path, &page.definition, 1000)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        vec![
            None,
            Some(Cell::Text("NULL".into())),
            Some(Cell::Binary(vec![0, 255])),
            None
        ]
    );
    assert_eq!(load(&mut db, &c, request("sample")).await?.total, 0);
    let changes = rows
        .into_iter()
        .map(|values| RowChange::Insert { values })
        .collect();
    assert_eq!(save(&mut db, &c, "sample", changes).await?.total, 2);
    std::fs::write(&path, "1\tx\tinvalid-hex\n")?;
    assert!(tomato_services::files::import_table_rows(&path, &page.definition, 1000).is_err());
    Ok(())
}

#[tokio::test]
async fn deferred_commit_constraint_failure_does_not_leave_pending_inserts() -> Result<()> {
    let mut db = Databases::default();
    let c = Connection::new("SQLITE");
    db.query(&c, "PRAGMA foreign_keys=ON; CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);").await?;
    assert!(
        save(
            &mut db,
            &c,
            "child",
            vec![RowChange::Insert {
                values: vec![Some(Cell::Integer(1)), Some(Cell::Integer(99))]
            }]
        )
        .await
        .is_err()
    );
    assert_eq!(load(&mut db, &c, request("child")).await?.total, 0);
    // Subsequent work must start clean rather than inheriting a failed savepoint.
    db.query(&c, "INSERT INTO parent VALUES(99)").await?;
    assert_eq!(
        save(
            &mut db,
            &c,
            "child",
            vec![RowChange::Insert {
                values: vec![Some(Cell::Integer(1)), Some(Cell::Integer(99))]
            }]
        )
        .await?
        .total,
        1
    );
    Ok(())
}
