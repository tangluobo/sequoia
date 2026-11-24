use super::*;
fn source(directory: &std::path::Path, text: &str) -> Result<Arc<Prepared>> {
    let path = directory.join("input.csv");
    std::fs::write(&path, text)?;
    let source = Source {
        format: Format::Csv,
        files: vec![path],
        delimiter: ',',
        ..Default::default()
    };
    let output = tempfile::tempdir()?;
    let catalog = crate::data_import::materialize(&source, output.path(), &Control::default())?;
    Ok(Arc::new(Prepared {
        tables: catalog.tables,
        directory: output,
        source,
    }))
}
fn connection(directory: &std::path::Path) -> Connection {
    let mut c = Connection::new("SQLITE");
    c.set(
        "database",
        directory
            .join("owned.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    c
}

#[tokio::test]
async fn data_import_rejects_sqlite_integer_overflow_without_coercing_it_to_lossy_real()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut db = Databases::default();
    db.query(&c, "CREATE TABLE target(id INTEGER PRIMARY KEY,note TEXT)")
        .await?;
    let prepared = source(dir.path(), "id,note\n18446744073709551615,exact\n")?;
    let table = TableId {
        schema: "main".into(),
        name: "target".into(),
    };
    let def = definition(&c, c.text("database"), &table, &Control::default()).await?;
    let target = defaults(0, &prepared.tables[0], table, Some(def));
    let report = import(
        c.clone(),
        prepared,
        target,
        Options::default(),
        Control::default(),
    )
    .await?;
    assert_eq!(report.failed, 1);
    assert_eq!(report.committed, 0);
    assert!(report.error.as_deref().is_some_and(|e| e.contains("64")));
    assert_eq!(
        db.query(&c, "SELECT COUNT(*) FROM target").await?[0].value(0, 0),
        Some("0")
    );
    Ok(())
}

#[tokio::test]
async fn data_import_cancel_copy_rolls_back_and_batch_mode_retains_only_committed_rows()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut db = Databases::default();
    db.query(&c,"CREATE TABLE target(id INTEGER PRIMARY KEY,note TEXT); INSERT INTO target VALUES(-1,'original')").await?;
    let mut text = "id,note\n".to_owned();
    for n in 0..10003 {
        text.push_str(&format!("{n},source\n"));
    }
    let prepared = source(dir.path(), &text)?;
    let table = TableId {
        schema: "main".into(),
        name: "target".into(),
    };
    for batch in [false, true] {
        let def = definition(&c, c.text("database"), &table, &Control::default()).await?;
        let target = defaults(0, &prepared.tables[0], table.clone(), Some(def));
        let control = Control::default();
        let cancellation = control.clone();
        let selected = c.clone();
        let source = prepared.clone();
        let job = tokio::spawn(async move {
            import(
                selected,
                source,
                target,
                Options {
                    mode: if batch { Mode::Append } else { Mode::Copy },
                    transaction: !batch,
                    batch_size: 10,
                    ..Default::default()
                },
                control,
            )
            .await
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if cancellation.read(|p| p.records >= 35).unwrap_or(false) {
                cancellation.cancel();
                break;
            }
            ensure!(
                !job.is_finished() && std::time::Instant::now() < deadline,
                "导入未能在测试中途取消"
            );
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let report = tokio::time::timeout(std::time::Duration::from_secs(4), job).await???;
        assert!(report.cancelled);
        assert!(!report.uncertain);
        let result = db
            .query(
                &c,
                "SELECT COUNT(*),MIN(id),MIN(CASE WHEN id=-1 THEN note END) FROM target",
            )
            .await?;
        if batch {
            assert!(report.committed >= 30 && report.committed < 10003);
            assert_eq!(
                result[0].value(0, 0).unwrap().parse::<u64>()?,
                report.committed + 1
            );
        } else {
            assert_eq!(report.committed, 0);
            assert_eq!(result[0].value(0, 0), Some("1"));
        }
        assert_eq!(result[0].value(0, 2), Some("original"));
    }
    Ok(())
}

#[tokio::test]
async fn data_import_changed_schema_and_nonunique_import_keys_fail_without_partial_writes()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut db = Databases::default();
    db.query(
        &c,
        "CREATE TABLE target(id INTEGER,note TEXT);INSERT INTO target VALUES(1,'a'),(1,'b')",
    )
    .await?;
    let prepared = source(dir.path(), "id,note\n1,changed\n")?;
    let table = TableId {
        schema: "main".into(),
        name: "target".into(),
    };
    let def = definition(&c, c.text("database"), &table, &Control::default()).await?;
    let mut target = defaults(0, &prepared.tables[0], table, Some(def));
    target.fields[0].key = true;
    let report = import(
        c.clone(),
        prepared.clone(),
        target.clone(),
        Options {
            mode: Mode::Update,
            ..Default::default()
        },
        Control::default(),
    )
    .await?;
    assert!(report.error.as_deref().is_some_and(|e| e.contains("唯一")));
    assert_eq!(report.committed, 0);
    db.query(&c, "ALTER TABLE target ADD COLUMN changed TEXT")
        .await?;
    let report = import(
        c.clone(),
        prepared,
        target,
        Options::default(),
        Control::default(),
    )
    .await?;
    assert!(report.error.as_deref().is_some_and(|e| e.contains("结构")));
    assert_eq!(report.committed, 0);
    assert_eq!(
        db.query(&c, "SELECT note FROM target ORDER BY note")
            .await?[0]
            .value(0, 0),
        Some("a")
    );
    Ok(())
}
async fn import(
    connection: Connection,
    prepared: Arc<Prepared>,
    target: Target,
    options: Options,
    control: Control,
) -> Result<Report> {
    let request = Request {
        database: connection.text("database").into(),
        connection,
        prepared,
        targets: vec![target],
        options,
    };
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || run(request, control, handle)).await?
}
#[tokio::test]
async fn data_import_executes_all_six_modes_with_typed_mapping_and_affected_counts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut db = Databases::default();
    db.query(
        &c,
        "CREATE TABLE target(id INTEGER PRIMARY KEY, note TEXT NOT NULL)",
    )
    .await?;
    let table = TableId {
        schema: "main".into(),
        name: "target".into(),
    };
    let prepared = source(dir.path(), "id,note\n1,changed\n3,new\n")?;
    for mode in Mode::ALL {
        db.query(
            &c,
            "DELETE FROM target; INSERT INTO target VALUES(1,'old'),(2,'keep')",
        )
        .await?;
        let prepared = if mode == Mode::Append {
            source(dir.path(), "id,note\n3,new\n4,added\n")?
        } else {
            prepared.clone()
        };
        let definition = definition(&c, c.text("database"), &table, &Control::default()).await?;
        let mut target = defaults(0, &prepared.tables[0], table.clone(), Some(definition));
        target.fields[0].key = true;
        let actual = import(
            c.clone(),
            prepared,
            target,
            Options {
                mode,
                ..Default::default()
            },
            Control::default(),
        )
        .await?;
        let rows = db
            .query(&c, "SELECT id,note FROM target ORDER BY id")
            .await?
            .remove(0);
        assert!(actual.error.is_none(), "{:?}", actual.error);
        assert_eq!(actual.read, 2);
        let expected: Vec<Vec<Option<String>>> = match mode {
            Mode::Append => vec![
                vec![Some("1".into()), Some("old".into())],
                vec![Some("2".into()), Some("keep".into())],
                vec![Some("3".into()), Some("new".into())],
                vec![Some("4".into()), Some("added".into())],
            ],
            Mode::Update => vec![
                vec![Some("1".into()), Some("changed".into())],
                vec![Some("2".into()), Some("keep".into())],
            ],
            Mode::AppendUpdate => vec![
                vec![Some("1".into()), Some("changed".into())],
                vec![Some("2".into()), Some("keep".into())],
                vec![Some("3".into()), Some("new".into())],
            ],
            Mode::SkipDuplicates => vec![
                vec![Some("1".into()), Some("old".into())],
                vec![Some("2".into()), Some("keep".into())],
                vec![Some("3".into()), Some("new".into())],
            ],
            Mode::Delete => vec![vec![Some("2".into()), Some("keep".into())]],
            Mode::Copy => vec![
                vec![Some("1".into()), Some("changed".into())],
                vec![Some("3".into()), Some("new".into())],
            ],
        };
        let actual = (0..rows.rows.len())
            .map(|r| {
                (0..rows.columns.len())
                    .map(|c| rows.value(r, c).map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{}", mode.label());
    }
    Ok(())
}
#[tokio::test]
async fn data_import_rolls_back_failed_table_and_continues_only_with_explicit_error_option()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut db = Databases::default();
    db.query(&c,"CREATE TABLE target(id INTEGER PRIMARY KEY, note TEXT NOT NULL); INSERT INTO target VALUES(1,'old')").await?;
    let table = TableId {
        schema: "main".into(),
        name: "target".into(),
    };
    let prepared = source(dir.path(), "id,note\n3,new\n1,duplicate\n4,\\N\n5,last\n")?;
    let def = definition(&c, c.text("database"), &table, &Control::default()).await?;
    let target = defaults(0, &prepared.tables[0], table.clone(), Some(def));
    let stopped = import(
        c.clone(),
        prepared.clone(),
        target.clone(),
        Options::default(),
        Control::default(),
    )
    .await?;
    assert_eq!(stopped.committed, 0);
    assert_eq!(stopped.inserted, 0);
    assert!(stopped.error.is_some());
    assert_eq!(
        db.query(&c, "SELECT COUNT(*) FROM target").await?[0].value(0, 0),
        Some("1")
    );
    let continued = import(
        c.clone(),
        prepared,
        target,
        Options {
            continue_on_error: true,
            ..Default::default()
        },
        Control::default(),
    )
    .await?;
    assert!(continued.error.is_none());
    assert_eq!(continued.inserted, 2);
    assert_eq!(continued.failed, 2);
    assert_eq!(continued.committed, 2);
    assert_eq!(
        db.query(&c, "SELECT COUNT(*) FROM target").await?[0].value(0, 0),
        Some("3")
    );
    Ok(())
}
#[tokio::test]
async fn data_import_creates_table_imports_beyond_ui_cap_and_preserves_exact_decimal_defaults()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let c = connection(dir.path());
    let mut text = "id,note,amount\n".to_owned();
    for i in 0..5003 {
        text.push_str(&format!("{i},中文,12345678901234567890.123456\n"));
    }
    let prepared = source(dir.path(), &text)?;
    let table = TableId {
        schema: "main".into(),
        name: "created\"table".into(),
    };
    let mut target = defaults(0, &prepared.tables[0], table.clone(), None);
    target.fields.push(Mapping {
        enabled: true,
        source: None,
        source_name: None,
        default: Some("constant".into()),
        target: "extra".into(),
        data_type: DataType::Text,
        nullable: false,
        key: false,
    });
    let report = import(
        c.clone(),
        prepared,
        target,
        Options::default(),
        Control::default(),
    )
    .await?;
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.inserted, 5003);
    let mut db = Databases::default();
    let rows = db
        .query(
            &c,
            "SELECT COUNT(*),MIN(amount),MAX(extra) FROM \"created\"\"table\"",
        )
        .await?;
    assert_eq!(rows[0].value(0, 0), Some("5003"));
    assert_eq!(rows[0].value(0, 1), Some("12345678901234567890.123456"));
    assert_eq!(rows[0].value(0, 2), Some("constant"));
    Ok(())
}
