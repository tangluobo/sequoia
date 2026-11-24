use std::io::Write;
use tomato_core::backup::{Definition, Metadata};
use tomato_services::backup::{Archive, Page, Writer};
fn source() -> anyhow::Result<Archive> {
    Archive::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/backup/original.nb3"),
    )
}
#[test]
fn checksum_verification_observes_cancellation_between_reads_and_keeps_reader_reusable()
-> anyhow::Result<()> {
    let mut archive = source()?;
    let meta: Metadata = archive.json("meta.json", false)?;
    let entry = &meta.objects[0].metadata;
    let mut checks = 0;
    let result = archive.verify_with(entry, || {
        checks += 1;
        anyhow::ensure!(checks < 2, "fixture checksum cancellation");
        Ok(())
    });
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("fixture checksum cancellation")
    );
    assert_eq!(checks, 2);
    archive.verify(entry)?;
    Ok(())
}
#[test]
fn original_java_nb3_reads_checksums_metadata_and_embedded_record_separators_incrementally()
-> anyhow::Result<()> {
    let mut archive = source()?;
    let meta: Metadata = archive.json("meta.json", false)?;
    assert_eq!(meta.objects.len(), 1);
    assert_eq!(meta.database_type, "MYSQL");
    archive.verify(&meta.objects[0].metadata)?;
    let definition: Definition = archive.json(&meta.objects[0].metadata.filename, true)?;
    assert_eq!(definition.fields.as_deref().unwrap(), ["id", "value"]);
    let mut rows = Vec::new();
    for page in definition.data.unwrap() {
        archive.verify(&page)?;
        archive.rows(&page.filename, |row| {
            rows.push(row.to_owned());
            Ok(())
        })?;
    }
    assert_eq!(
        rows,
        [
            "(1, '中文 O\\'Reilly')",
            "(2, 'one\u{1e}\ntwo')",
            "(3, 'line\nnext')"
        ]
    );
    Ok(())
}
#[test]
fn native_nb3_writer_keeps_original_format_and_dropped_work_preserves_existing_backup()
-> anyhow::Result<()> {
    let mut original = source()?;
    let mut meta: Metadata = original.json("meta.json", false)?;
    let mut definition: Definition = original.json(&meta.objects[0].metadata.filename, true)?;
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("native.nb3");
    let mut writer = Writer::new(directory.path())?;
    let mut page = Page::new(directory.path())?;
    page.write_all("(1, '第一行')\u{1e}\n(2, 'second')".as_bytes())?;
    let data = writer.page("native.data.00000.sql.gz", page)?;
    definition.data = Some(vec![data]);
    meta.objects[0].metadata = writer.json("native.meta.json.gz", &definition)?;
    meta.objects[0].rows = "2".into();
    writer.finish(&target, &meta)?;
    let before = std::fs::read(&target)?;
    let mut native = Archive::open(&target)?;
    let native_meta: Metadata = native.json("meta.json", false)?;
    native.verify(&native_meta.objects[0].metadata)?;
    let native_definition: Definition =
        native.json(&native_meta.objects[0].metadata.filename, true)?;
    let mut count = 0;
    for data in native_definition.data.unwrap() {
        native.verify(&data)?;
        native.rows(&data.filename, |_| {
            count += 1;
            Ok(())
        })?;
    }
    assert_eq!(count, 2);
    drop(native);
    {
        let _cancelled = Writer::new(directory.path())?;
        let mut partial = Page::new(directory.path())?;
        partial.write_all(b"unfinished")?;
    }
    assert_eq!(std::fs::read(&target)?, before);
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    meta.comment = "\"".repeat(8 * 1024 * 1024);
    let oversized = Writer::new(directory.path())?;
    assert!(oversized.finish(&target, &meta).is_err());
    assert_eq!(std::fs::read(&target)?, before);
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}
#[tokio::test]
async fn streaming_backup_scalar_codec_preserves_controls_unicode_binary_and_rejects_numeric_sql()
-> anyhow::Result<()> {
    use tomato_core::backup::sql::{Value, literal};
    let text = "中文 ' quote \\ path\u{1e}\n\0😀";
    let mut encoded = Vec::new();
    literal(&mut encoded, "SQLITE", Value::Text(text))?;
    let text_sql = String::from_utf8(encoded)?;
    let mut binary = Vec::new();
    literal(&mut binary, "SQLITE", Value::Binary(&[0, 255, 30, 39, 92]))?;
    let binary_sql = String::from_utf8(binary)?;
    let mut database = tomato_services::database::Databases::default();
    let connection = tomato_core::Connection::new("SQLITE");
    let rows = database
        .query(&connection, &format!("SELECT {text_sql},HEX({binary_sql})"))
        .await?;
    assert_eq!(rows[0].value(0, 0), Some(text));
    assert_eq!(rows[0].value(0, 1), Some("00FF1E275C"));
    assert!(
        literal(
            &mut Vec::new(),
            "MYSQL",
            Value::Number("1); DROP TABLE items;--")
        )
        .is_err()
    );
    let mut precise = Vec::new();
    literal(
        &mut precise,
        "MYSQL",
        Value::Number("12345678901234567890.0012300"),
    )?;
    assert_eq!(precise, b"12345678901234567890.0012300");
    Ok(())
}
#[test]
fn backup_progress_keeps_bounded_logs_and_cross_thread_cancellation() -> anyhow::Result<()> {
    let control = tomato_services::backup::Control::default();
    control.start(2);
    control.records(1000);
    control.object();
    for _ in 0..1000 {
        control.log(&"字".repeat(1000));
    }
    assert_eq!(
        control.read(|progress| (progress.objects, progress.total, progress.records)),
        Some((1, 2, 1000))
    );
    assert!(
        control
            .read(|progress| progress.logs.len() <= 256
                && progress.logs.iter().map(String::len).sum::<usize>() <= 64 * 1024)
            .unwrap()
    );
    let worker = control.clone();
    std::thread::spawn(move || worker.cancel()).join().unwrap();
    assert!(control.check().is_err());
    Ok(())
}
#[test]
fn backup_definition_data_entries_stream_into_a_source_compatible_json_array() -> anyhow::Result<()>
{
    use tomato_services::backup::DefinitionPage;
    let mut original = source()?;
    let mut meta: Metadata = original.json("meta.json", false)?;
    let mut definition: Definition = original.json(&meta.objects[0].metadata.filename, true)?;
    definition.data = None;
    let directory = tempfile::tempdir()?;
    let mut writer = Writer::new(directory.path())?;
    let mut header = DefinitionPage::new(directory.path(), &definition)?;
    for index in 0..5000 {
        header.data(&tomato_core::backup::Entry {
            filename: format!("{index}.data.sql.gz"),
            checksum: "1234567890".into(),
        })?;
    }
    meta.objects[0].metadata = header.finish(&mut writer, "streamed.meta.json.gz")?;
    let path = directory.path().join("streamed.nb3");
    writer.finish(&path, &meta)?;
    let mut archive = Archive::open(&path)?;
    let decoded: Definition = archive.json("streamed.meta.json.gz", true)?;
    assert_eq!(decoded.data.as_ref().unwrap().len(), 5000);
    assert_eq!(decoded.data.unwrap()[4999].filename, "4999.data.sql.gz");
    assert_eq!(decoded.ddl, definition.ddl);
    Ok(())
}
