use super::*;
#[test]
fn quoted_csv_retains_multiline_unicode_exact_numbers_nulls_and_all_records() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("records.csv");
    let mut file = File::create(&path)?;
    writeln!(file, "id,postal,amount,note,missing")?;
    for id in 0..2305 {
        writeln!(
            file,
            "{id},00123,12345678901234567890.123456,\"中文,🙂\nline \"\"two\"\"\",\\N"
        )?;
    }
    drop(file);
    let source = Source {
        format: Format::Csv,
        files: vec![path],
        delimiter: ',',
        ..Default::default()
    };
    let output = tempfile::tempdir()?;
    let catalog = materialize(&source, output.path(), &Control::default())?;
    assert_eq!(catalog.tables[0].rows, 2305);
    assert_eq!(catalog.tables[0].preview.len(), PREVIEW_ROWS);
    let row = &catalog.tables[0].preview[0];
    assert_eq!(row[1], Cell::Text("00123".into()));
    assert_eq!(row[2], Cell::Text("12345678901234567890.123456".into()));
    assert_eq!(row[3], Cell::Text("中文,🙂\nline \"two\"".into()));
    assert_eq!(row[4], Cell::Null);
    let prepared = Prepared {
        tables: catalog.tables,
        directory: output,
        source,
    };
    let mut count = 0;
    prepared.each(0, &Control::default(), |_, _| {
        count += 1;
        Ok(())
    })?;
    assert_eq!(count, 2305);
    Ok(())
}
#[test]
fn profile_never_contains_database_or_odbc_passwords_or_connection_strings() -> Result<()> {
    let mut source = Source {
        access_password: "access-secret".into(),
        ..Default::default()
    };
    source.odbc.password = "odbc-secret".into();
    source.odbc.connection_string = "Driver={X};UID=user;PWD=hidden-secret".into();
    let profile = Profile::redacted(&source, &[], &Options::default());
    let text = serde_json::to_string(&profile)?;
    for secret in ["access-secret", "odbc-secret", "hidden-secret", "PWD"] {
        assert!(!text.contains(secret));
        assert!(!format!("{source:?}").contains(secret));
    }
    Ok(())
}

#[test]
fn oversized_unquoted_or_quoted_records_are_rejected_before_unbounded_allocation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (ext, prefix, suffix, format) in [
        ("csv", "a\n\"", "\"\n", Format::Csv),
        ("json", "[{\"a\":\"", "\"}]", Format::Json),
        ("xml", "<root><row><a>", "</a></row></root>", Format::Xml),
    ] {
        let path = dir.path().join(format!("large.{ext}"));
        let mut file = File::create(&path)?;
        file.write_all(prefix.as_bytes())?;
        let chunk = [b'a'; 64 * 1024];
        for _ in 0..260 {
            file.write_all(&chunk)?;
        }
        file.write_all(suffix.as_bytes())?;
        drop(file);
        let source = Source {
            format,
            files: vec![path],
            first_row: 1,
            ..Default::default()
        };
        let out = tempfile::tempdir()?;
        let error = materialize(&source, out.path(), &Control::default()).unwrap_err();
        assert!(format!("{error:#}").contains("16 MiB"), "{error:#}");
    }
    Ok(())
}
#[test]
#[ignore = "requires the independent Paradox 3/4/5/7 and MB public reference fixtures"]
fn independent_paradox_versions_blobs_dates_and_nulls() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("TOMATO_TEST_PARADOX_DIRECTORY")
            .context("set TOMATO_TEST_PARADOX_DIRECTORY")?,
    );
    for filename in [
        "date35.db",
        "date4.db",
        "date5.db",
        "date7.db",
        "long.db",
        "logical.db",
        "time.db",
        "timestamp.db",
        "bytes.db",
        "memo.db",
        "graphic.db",
    ] {
        let source = Source {
            format: Format::Paradox,
            files: vec![root.join(filename)],
            encoding: "自动".into(),
            first_row: 1,
            ..Default::default()
        };
        let out = tempfile::tempdir()?;
        let catalog = materialize(&source, out.path(), &Control::default())
            .with_context(|| filename.to_owned())?;
        assert!(!catalog.tables[0].fields.is_empty());
        assert!(catalog.tables[0].rows > 0);
        let prepared = Prepared {
            tables: catalog.tables,
            directory: out,
            source,
        };
        let mut n = 0;
        prepared.each(0, &Control::default(), |_, _| {
            n += 1;
            Ok(())
        })?;
        assert_eq!(n, prepared.tables[0].rows);
    }
    Ok(())
}
#[test]
fn json_path_and_xml_preserve_null_empty_binary_and_exact_numeric_text() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let json = dir.path().join("rows.json");
    std::fs::write(
        &json,
        r#"{"data":{"rows":[{"id":18446744073709551615,"amount":12345678901234567890.123456,"blank":"","missing":null},{"id":2,"later":true}]}}"#,
    )?;
    let source = Source {
        format: Format::Json,
        files: vec![json],
        json_path: "data.rows".into(),
        first_row: 1,
        ..Default::default()
    };
    let out = tempfile::tempdir()?;
    let catalog = materialize(&source, out.path(), &Control::default())?;
    let table = &catalog.tables[0];
    assert_eq!(table.rows, 2);
    assert_eq!(table.fields.len(), 5);
    assert_eq!(
        table.preview[0][0],
        Cell::Text("18446744073709551615".into())
    );
    assert_eq!(
        table.preview[0][1],
        Cell::Text("12345678901234567890.123456".into())
    );
    assert_eq!(table.preview[0][2], Cell::Text(String::new()));
    assert_eq!(table.preview[0][3], Cell::Null);
    assert_eq!(table.preview[0][4], Cell::Null);
    let xml = dir.path().join("rows.xml");
    std::fs::write(
        &xml,
        r#"<?xml version="1.0"?><records><record><field name="note">中文 &amp; 🙂<![CDATA[ <raw>]]></field><field name="blank"></field><field name="missing" null="true"/><field name="bytes" encoding="hex">0001ff</field></record></records>"#,
    )?;
    let source = Source {
        format: Format::Xml,
        files: vec![xml],
        first_row: 1,
        ..Default::default()
    };
    let out = tempfile::tempdir()?;
    let catalog = materialize(&source, out.path(), &Control::default())?;
    assert_eq!(catalog.tables[0].rows, 1);
    let row = &catalog.tables[0].preview[0];
    assert_eq!(row[0], Cell::Text("中文 & 🙂 <raw>".into()));
    assert_eq!(row[1], Cell::Text(String::new()));
    assert_eq!(row[2], Cell::Null);
    assert_eq!(row[3], Cell::Binary(vec![0, 1, 255]));
    Ok(())
}
#[test]
fn delimiter_encoding_and_record_range_are_applied_before_spooling() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("gbk.txt");
    let (bytes, _, errors) =
        encoding_rs::GBK.encode("说明\r\n编号;姓名\r\n1;张三\r\n2;李四\r\n3;王五\r\n");
    assert!(!errors);
    std::fs::write(&path, bytes)?;
    let source = Source {
        files: vec![path],
        encoding: "GBK".into(),
        delimiter: ';',
        header_row: 2,
        first_row: 4,
        last_row: Some(4),
        ..Default::default()
    };
    let out = tempfile::tempdir()?;
    let catalog = materialize(&source, out.path(), &Control::default())?;
    assert_eq!(catalog.tables[0].rows, 1);
    assert_eq!(catalog.tables[0].fields[1].name, "姓名");
    assert_eq!(catalog.tables[0].preview[0][1], Cell::Text("李四".into()));
    Ok(())
}

#[test]
fn quoted_null_markers_empty_strings_crlf_and_buffer_boundaries_remain_distinct() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("quoted.csv");
    let long = "a".repeat(65537);
    std::fs::write(
        &path,
        format!(
            "\r\na,b,c\r\n\\N,\"\\N\",\"\"\r\n\"first\r\nsecond\"\"quote\",{long},tail\r\n\"\",\"\",\"\"\r\nlast,\"ends\"\"quote\""
        ),
    )?;
    let source = Source {
        format: Format::Csv,
        files: vec![path],
        delimiter: ',',
        ..Default::default()
    };
    let out = tempfile::tempdir()?;
    let catalog = materialize(&source, out.path(), &Control::default())?;
    let t = &catalog.tables[0];
    assert_eq!(t.rows, 4);
    assert_eq!(
        t.preview[0],
        vec![
            Cell::Null,
            Cell::Text("\\N".into()),
            Cell::Text(String::new())
        ]
    );
    assert_eq!(t.preview[1][0], Cell::Text("first\r\nsecond\"quote".into()));
    assert_eq!(t.preview[1][1], Cell::Text(long));
    assert_eq!(t.preview[2], vec![Cell::Text(String::new()); 3]);
    assert_eq!(t.preview[3][1], Cell::Text("ends\"quote".into()));
    Ok(())
}
