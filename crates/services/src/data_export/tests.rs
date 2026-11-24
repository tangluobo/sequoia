use super::*;
use calamine::{Data, Reader};
use tomato_core::workbench::Column;
fn column(name: &str, kind: &str) -> Column {
    Column {
        name: name.into(),
        data_type: kind.into(),
        nullable: true,
        default: None,
        primary_key: 0,
        generated: false,
        auto_increment: false,
        binary: kind == "BLOB",
        type_schema: String::new(),
        type_name: String::new(),
    }
}
fn sample() -> Snapshot {
    Snapshot {
        schema: Schema {
            table: TableId {
                schema: "main".into(),
                name: "导出表".into(),
            },
            dialect: "SQLITE".into(),
            columns: vec![
                column("id", "INTEGER"),
                column("name", "TEXT"),
                column("note", "TEXT"),
                column("data", "BLOB"),
            ],
        },
        rows: vec![
            vec![
                Cell::Integer(i64::MAX),
                Cell::Text("中文,\"引号\"\r\n下一行".into()),
                Cell::Null,
                Cell::Binary(vec![0, 255, 17]),
            ],
            vec![
                Cell::Integer(2),
                Cell::Text("=1+1".into()),
                Cell::Text(String::new()),
                Cell::Binary(vec![]),
            ],
            vec![
                Cell::Integer(3),
                Cell::Text("NULL".into()),
                Cell::Text("\\N".into()),
                Cell::Null,
            ],
        ],
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}
fn export(
    format: Format,
    path: &Path,
    snapshot: Snapshot,
    handle: tokio::runtime::Handle,
) -> Result<Report> {
    run(
        Request {
            source: Source::Current(snapshot),
            path: path.into(),
            options: Options {
                format,
                delimiter: if format == Format::Csv { ',' } else { '\t' },
                ..Default::default()
            },
        },
        Control::default(),
        handle,
    )
}
#[test]
fn ten_formats_write_real_files_and_independent_excel_dbf_readers_accept_them() -> Result<()> {
    let runtime = runtime();
    let directory = tempfile::tempdir()?;
    for format in Format::ALL {
        let path = directory
            .path()
            .join(format!("export.{}", format.extension()));
        let report = export(format, &path, sample(), runtime.handle().clone())
            .with_context(|| format!("{format:?}"))?;
        assert_eq!(report.records, 3);
        assert!(path.metadata()?.len() > 0);
        match format {
            Format::Json => {
                let rows: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
                assert_eq!(rows[0]["id"].as_i64(), Some(i64::MAX));
                assert!(rows[0]["note"].is_null());
                assert_eq!(rows[1]["note"], "");
                assert_eq!(rows[2]["note"], "\\N");
                assert_eq!(rows[0]["data"], "00FF11");
            }
            Format::Xls | Format::Xlsx => {
                let mut book = calamine::open_workbook_auto(&path)?;
                let sheet = book.worksheet_range_at(0).unwrap()?;
                assert_eq!(sheet.height(), 4);
                assert_eq!(sheet.width(), 4);
                assert_eq!(sheet.get((1, 0)), Some(&Data::String(i64::MAX.to_string())));
                assert_eq!(
                    sheet.get((1, 1)),
                    Some(&Data::String("中文,\"引号\"\r\n下一行".into()))
                );
                assert_eq!(sheet.get((2, 1)), Some(&Data::String("=1+1".into())));
                assert_eq!(sheet.get((1, 2)), Some(&Data::Empty));
            }
            Format::Dbase => {
                let rows = dbase::read(&path)?;
                assert_eq!(rows.len(), 3);
                assert_eq!(
                    rows[0].get("id"),
                    Some(&dbase::FieldValue::Character(Some(i64::MAX.to_string())))
                );
                assert_eq!(
                    rows[0].get("name"),
                    Some(&dbase::FieldValue::Character(Some(
                        "中文,\"引号\"\r\n下一行".into()
                    )))
                );
            }
            Format::Access => {
                assert_eq!(&std::fs::read(&path)?[4..19], b"Standard ACE DB");
                let output = std::process::Command::new(env!("TOMATO_ACCESS_EXPORTER"))
                    .arg("--verify")
                    .arg(&path)
                    .output()?;
                ensure!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("\t3"));
            }
            Format::Xml => {
                let xml = std::fs::read_to_string(&path)?;
                let mut reader = quick_xml::Reader::from_str(&xml);
                loop {
                    if reader.read_event()? == quick_xml::events::Event::Eof {
                        break;
                    }
                }
                assert!(xml.contains("null=\"true\""));
            }
            Format::Sql => {
                let sql = std::fs::read_to_string(&path)?;
                runtime.block_on(async {
                    let mut c = Connection::new("SQLITE");
                    c.set(
                        "database",
                        directory
                            .path()
                            .join("restored.db")
                            .to_string_lossy()
                            .as_ref(),
                    );
                    let mut db = crate::Databases::default();
                    db.query(
                        &c,
                        "CREATE TABLE 导出表(id INTEGER,name TEXT,note TEXT,data BLOB)",
                    )
                    .await?;
                    db.query(&c, &sql).await?;
                    let results = db
                        .query(
                            &c,
                            "SELECT name,note,hex(data) FROM 导出表 ORDER BY id DESC",
                        )
                        .await?;
                    assert_eq!(results[0].rows[0][0], "中文,\"引号\"\r\n下一行");
                    assert!(results[0].is_null(0, 1));
                    assert_eq!(results[0].rows[0][2], "00FF11");
                    Ok::<_, anyhow::Error>(())
                })?;
            }
            _ => {}
        }
    }
    assert!(
        !std::fs::read_dir(directory.path())?
            .filter_map(Result::ok)
            .any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with(".tomato-export"))
    );
    Ok(())
}
#[test]
fn all_records_pass_page_limit_and_query_export_is_read_only_and_cancellable() -> Result<()> {
    let runtime = runtime();
    let directory = tempfile::tempdir()?;
    let mut c = Connection::new("SQLITE");
    c.set(
        "database",
        directory
            .path()
            .join("source.db")
            .to_string_lossy()
            .as_ref(),
    );
    runtime.block_on(async{let mut db=crate::Databases::default();db.query(&c,"CREATE TABLE source(id INTEGER PRIMARY KEY,txt TEXT,data BLOB);WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<5003) INSERT INTO source SELECT x,'第'||x||'行',x'00FF11' FROM n").await?;Ok::<_,anyhow::Error>(())})?;
    let source = Source::Table {
        connection: c.clone(),
        database: c.text("database").into(),
        table: TableId {
            schema: "main".into(),
            name: "source".into(),
        },
        sort: None,
        query: Default::default(),
    };
    let path = directory.path().join("all.json");
    let report = run(
        Request {
            source,
            path: path.clone(),
            options: Options {
                format: Format::Json,
                ..Default::default()
            },
        },
        Control::default(),
        runtime.handle().clone(),
    )?;
    assert_eq!(report.records, 5003);
    let rows: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
    assert_eq!(rows.as_array().unwrap().len(), 5003);
    assert_eq!(rows[5002]["txt"], "第5003行");
    assert_eq!(rows[5002]["data"], "00FF11");
    let schema = runtime.block_on(crate::database::export::schema(
        &c,
        c.text("database"),
        &TableId {
            schema: "main".into(),
            name: "source".into(),
        },
        &Control::default(),
    ))?;
    let source = Source::Query {
        connection: c.clone(),
        database: c.text("database").into(),
        sql: "SELECT * FROM source WHERE id>2000".into(),
        schema: schema.clone(),
    };
    let report = run(
        Request {
            source,
            path: directory.path().join("query.json"),
            options: Options {
                format: Format::Json,
                ..Default::default()
            },
        },
        Control::default(),
        runtime.handle().clone(),
    )?;
    assert_eq!(report.records, 3003);
    let source = Source::Query {
        connection: c.clone(),
        database: c.text("database").into(),
        sql: "SELECT * FROM source;DELETE FROM source".into(),
        schema,
    };
    assert!(
        run(
            Request {
                source,
                path: path.clone(),
                options: Options::default()
            },
            Control::default(),
            runtime.handle().clone()
        )
        .is_err()
    );
    let before = std::fs::read(&path)?;
    let control = Control::default();
    control.cancel();
    assert!(
        run(
            Request {
                source: Source::Current(sample()),
                path: path.clone(),
                options: Options::default()
            },
            control,
            runtime.handle().clone()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}
#[test]
fn excel_long_unicode_continuations_and_format_errors_preserve_existing_file() -> Result<()> {
    let runtime = runtime();
    let directory = tempfile::tempdir()?;
    let mut snapshot = sample();
    snapshot.rows[0][1] = Cell::Text("长文本😀".repeat(3000));
    let path = directory.path().join("long.xls");
    export(
        Format::Xls,
        &path,
        snapshot.clone(),
        runtime.handle().clone(),
    )?;
    let mut book = calamine::open_workbook_auto(&path)?;
    let sheet = book.worksheet_range_at(0).unwrap()?;
    assert_eq!(
        sheet.get((1, 1)),
        Some(&Data::String("长文本😀".repeat(3000)))
    );
    let before = std::fs::read(&path)?;
    assert!(export(Format::Dbase, &path, snapshot, runtime.handle().clone()).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}
#[test]
fn changed_destination_and_mid_stream_cancellation_never_publish_partial_files() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let runtime = runtime();
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("existing.json");
    std::fs::write(&path, b"original")?;
    let notifications = Arc::new(AtomicUsize::new(0));
    let changed_path = path.clone();
    let control = Control::new(move || {
        if notifications.fetch_add(1, Ordering::SeqCst) == 1 {
            std::fs::write(&changed_path, b"externally changed, keep this file").unwrap();
        }
    });
    let error = run(
        Request {
            source: Source::Current(sample()),
            path: path.clone(),
            options: Options {
                format: Format::Json,
                ..Default::default()
            },
        },
        control,
        runtime.handle().clone(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("其他程序修改"));
    assert_eq!(std::fs::read(&path)?, b"externally changed, keep this file");
    let mut snapshot = sample();
    snapshot.rows = vec![
        vec![
            Cell::Integer(1),
            Cell::Text("x".repeat(4096)),
            Cell::Null,
            Cell::Null
        ];
        10000
    ];
    let control = Control::default();
    let worker_control = control.clone();
    let handle = runtime.handle().clone();
    let output = path.clone();
    let worker = std::thread::spawn(move || {
        run(
            Request {
                source: Source::Current(snapshot),
                path: output,
                options: Options {
                    format: Format::Json,
                    ..Default::default()
                },
            },
            worker_control,
            handle,
        )
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while control.read(|p| p.records).unwrap_or(0) < 256 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    control.cancel();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(std::fs::read(&path)?, b"externally changed, keep this file");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}
