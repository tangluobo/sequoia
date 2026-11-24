use super::*;
use crate::app::tests::native_bridge::Bridge;
use tomato_core::workbench::Cell;
#[test]
fn import_native_radio_buttons_navigation_and_validation() -> anyhow::Result<()> {
    let ctx = Context::default();
    let dir = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    app.open_data_import(Connection::new("SQLITE"), "main".into(), None);
    let bridge = Bridge::new(&ctx);
    bridge.frame(&ctx, vec![], |c| app.data_import_dialog(c));
    assert_eq!(
        app.data_import.as_ref().unwrap().source.format,
        Format::Text
    );
    for format in Format::ALL {
        let p = bridge.text(format.label())?;
        bridge.click(&ctx, p, |c| app.data_import_dialog(c));
        assert_eq!(app.data_import.as_ref().unwrap().source.format, format);
    }
    let p = bridge.text(Format::Csv.label())?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    let p = bridge.text("下一步 >")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    assert_eq!(app.data_import.as_ref().unwrap().step, 1);
    let p = bridge.text("下一步 >")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    assert_eq!(app.data_import.as_ref().unwrap().step, 1);
    assert!(app.data_import.as_ref().unwrap().error.is_some());
    app.data_import.as_mut().unwrap().files =
        dir.path().join("input.csv").to_string_lossy().into_owned();
    let p = bridge.text("下一步 >")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    assert_eq!(app.data_import.as_ref().unwrap().step, 2);
    let p = bridge.text("< 上一步")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    assert_eq!(app.data_import.as_ref().unwrap().step, 1);
    let p = bridge.text("取消")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    assert!(app.data_import.is_none());
    assert!(!dir.path().join("connections.json").exists());
    Ok(())
}
#[test]
fn native_import_mapping_execution_and_omitted_defaults_reach_database() -> anyhow::Result<()> {
    let ctx = Context::default();
    let dir = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    let mut c = Connection::new("SQLITE");
    c.set(
        "database",
        dir.path().join("owned.sqlite").to_string_lossy().as_ref(),
    );
    app.runtime.as_ref().unwrap().block_on(async{tomato_services::Databases::default().query(&c,"CREATE TABLE records(id INTEGER PRIMARY KEY,note TEXT,other TEXT DEFAULT 'server-default')").await})?;
    let directory = tempfile::tempdir()?;
    let spool = "source.rows";
    let rows = vec![
        vec![Cell::Text("1".into()), Cell::Text("中文🙂".into())],
        vec![Cell::Text("2".into()), Cell::Null],
    ];
    use std::io::Write;
    let mut file = std::fs::File::create_new(directory.path().join(spool))?;
    for r in &rows {
        serde_json::to_writer(&mut file, r)?;
        writeln!(file)?;
    }
    drop(file);
    let table = SourceTable {
        name: "input".into(),
        origin: "owned".into(),
        fields: vec![
            Field {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Field {
                name: "note".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        rows: 2,
        preview: rows,
        spool: spool.into(),
    };
    let target = service::defaults(
        0,
        &table,
        TableId {
            schema: "main".into(),
            name: "records".into(),
        },
        None,
    );
    let prepared = Arc::new(Prepared {
        tables: vec![table],
        directory,
        source: Source::default(),
    });
    let mut d = Draft::new(c.clone(), "main".into(), None, &ctx);
    d.step = 3;
    d.prepared = Some(prepared);
    d.targets = vec![Target {
        create: false,
        ..target
    }];
    app.data_import = Some(d);
    let bridge = Bridge::new(&ctx);
    bridge.frame(&ctx, vec![], |c| app.data_import_dialog(c));
    let p = bridge.text("下一步 >")?;
    bridge.click(&ctx, p, |c| app.data_import_dialog(c));
    wait(&mut app, &bridge, &ctx)?;
    assert_eq!(app.data_import.as_ref().unwrap().step, 4);
    let target = &mut app.data_import.as_mut().unwrap().targets[0];
    target.fields.push(Mapping {
        enabled: true,
        source: None,
        source_name: None,
        default: None,
        target: "other".into(),
        data_type: DataType::Text,
        nullable: true,
        key: false,
    });
    for _ in 0..3 {
        bridge.frame(&ctx, vec![], |c| app.data_import_dialog(c));
    }
    let id = bridge.text("id")?;
    let note = bridge.text("note")?;
    assert!(
        ((note.y - id.y) - 26.).abs() < 0.1,
        "mapping rows changed height: {id:?} {note:?}"
    );
    bridge.click(&ctx, id, |c| app.data_import_dialog(c));
    let note_open = bridge.text("note")?;
    assert!(
        (note_open.y - note.y).abs() < 0.1,
        "opening a field selector moved the next row"
    );
    bridge.click(&ctx, id, |c| app.data_import_dialog(c));
    assert!(
        app.data_import.as_ref().unwrap().targets[0].fields[2]
            .default
            .is_none()
    );
    for expected in [5, 6, 7] {
        let p = bridge.text(if expected == 7 {
            "开始"
        } else {
            "下一步 >"
        })?;
        bridge.click(&ctx, p, |c| app.data_import_dialog(c));
        assert_eq!(app.data_import.as_ref().unwrap().step, expected);
    }
    wait(&mut app, &bridge, &ctx)?;
    let d = app.data_import.as_ref().unwrap();
    assert!(d.error.is_none(), "{:?}", d.error);
    let r = d.report.as_ref().unwrap();
    assert_eq!((r.inserted, r.committed, r.failed), (2, 2, 0));
    let result = app.runtime.as_ref().unwrap().block_on(async {
        tomato_services::Databases::default()
            .query(&c, "SELECT note,other FROM records ORDER BY id")
            .await
    })?;
    assert_eq!(result[0].value(0, 0), Some("中文🙂"));
    assert_eq!(result[0].value(0, 1), Some("server-default"));
    assert_eq!(result[0].value(1, 1), Some("server-default"));
    assert!(!dir.path().join("connections.json").exists());
    Ok(())
}

#[test]
fn saved_profiles_match_source_identity_when_files_or_sheets_are_reordered() {
    let source = |name: &str, origin: &str| SourceTable {
        name: name.into(),
        origin: origin.into(),
        fields: Vec::new(),
        rows: 0,
        preview: Vec::new(),
        spool: String::new(),
    };
    let a = source("Sheet A", "book.xlsx");
    let b = source("Sheet B", "book.xlsx");
    let targets = vec![
        service::defaults(
            0,
            &a,
            TableId {
                schema: "main".into(),
                name: "destination_a".into(),
            },
            None,
        ),
        service::defaults(
            1,
            &b,
            TableId {
                schema: "main".into(),
                name: "destination_b".into(),
            },
            None,
        ),
    ];
    let profile = Profile::redacted(&Source::default(), &targets, &Options::default());
    assert_eq!(
        saved_target(&profile, &b, 0).unwrap().table.name,
        "destination_b"
    );
    assert_eq!(
        saved_target(&profile, &a, 1).unwrap().table.name,
        "destination_a"
    );
    assert!(saved_target(&profile, &source("Sheet A", "changed.xlsx"), 0).is_none());
    assert!(profile.targets.iter().all(|t| t.expected.is_none()));
}
fn wait(app: &mut TomatoApp, bridge: &Bridge, ctx: &Context) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while app.data_import.as_ref().unwrap().busy() {
        anyhow::ensure!(std::time::Instant::now() < deadline, "导入后台任务超时");
        std::thread::sleep(std::time::Duration::from_millis(10));
        bridge.frame(ctx, vec![], |c| app.data_import_dialog(c));
    }
    Ok(())
}
