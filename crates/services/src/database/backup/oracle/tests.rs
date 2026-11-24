use super::*;
use crate::backup::{Archive, Repository};
use sqlx::Connection as _;
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
        type_name: kind.into(),
    }
}
fn metadata(entry: backup::Entry) -> Metadata {
    Metadata {
        meta_version: "30100".into(),
        database_type: "ORACLE".into(),
        catalog: String::new(),
        schema: "main".into(),
        start_time: "0".into(),
        end_time: "0".into(),
        encryption: "None".into(),
        comment: String::new(),
        objects: vec![Object {
            uuid: "fixture".into(),
            kind: "Table".into(),
            name: "items".into(),
            rows: "1".into(),
            metadata: entry,
        }],
    }
}
fn definition(columns: &[Column]) -> Definition {
    Definition {
        meta_version: "30100".into(),
        name: "items".into(),
        kind: "Table".into(),
        ddl: "CREATE TABLE \"main\".\"items\" (\"id\" NUMBER)".into(),
        sub_ddl: None,
        auto_increment: None,
        fields: Some(columns.iter().map(|c| c.name.clone()).collect()),
        trigger_ddl: None,
        index_ddl: None,
        data: None,
        postgres: None,
        oracle: Some(OracleDefinition {
            schema: "main".into(),
            standard_strings: true,
        }),
    }
}
#[test]
fn oracle_nb3_roundtrip_keeps_large_binary_precision_empty_lobs_nulls_and_embedded_separators()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let columns = vec![
        column("id", "NUMBER"),
        column("text", "CLOB"),
        column("binary", "BLOB"),
        column("empty_text", "CLOB"),
        column("empty_binary", "BLOB"),
        column("missing", "VARCHAR2"),
    ];
    let cells = vec![
        Cell::Text("12345678901234567890.123456789012345678".into()),
        Cell::Text("中文 O'Reilly\u{1e}\nC:\\".into()),
        Cell::Binary((0..5001).map(|i| (i % 251) as u8).collect()),
        Cell::Text(String::new()),
        Cell::Binary(vec![]),
        Cell::Null,
    ];
    let mut writer = Writer::new(root.path())?;
    let mut page = Page::new(root.path())?;
    page.record(|output| {
        output.write_all(b"(")?;
        for (i, (cell, column)) in cells.iter().zip(&columns).enumerate() {
            if i > 0 {
                output.write_all(b", ")?;
            }
            write_cell(output, cell, column)?;
        }
        output.write_all(b")")?;
        Ok(())
    })?;
    let mut definition = definition(&columns);
    definition.data = Some(vec![writer.page("data.sql.gz", page)?]);
    let entry = writer.json("definition.json.gz", &definition)?;
    let path = root.path().join("fixture.nb3");
    writer.finish(&path, &metadata(entry))?;
    let mut archive = Archive::open(&path)?;
    let meta: Metadata = archive.json("meta.json", false)?;
    archive.verify(&meta.objects[0].metadata)?;
    let definition: Definition = archive.json(&meta.objects[0].metadata.filename, true)?;
    assert!(definition.oracle.unwrap().standard_strings);
    let data = &definition.data.unwrap()[0];
    archive.verify(data)?;
    let mut rows = archive.records_with_strings(&data.filename, true)?;
    let values = super::super::oracle_values::parse(
        rows.next_record()?.context("missing data")?,
        true,
        columns.len(),
    )?;
    assert_eq!(values, cells);
    assert!(rows.next_record()?.is_none());
    let described = tomato_core::workbench::TableDefinition {
        table: TableId {
            schema: "main".into(),
            name: "items".into(),
        },
        columns,
        comments: Default::default(),
        view: false,
        transactional: true,
    };
    let statement = workbench::plan::build(
        "ORACLE",
        &described,
        &tomato_core::workbench::RowChange::Insert {
            values: values.into_iter().map(Some).collect(),
        },
    )?
    .unwrap();
    assert!(statement.sql.contains("EMPTY_CLOB()") && statement.sql.contains("EMPTY_BLOB()"));
    assert_eq!(statement.params.len(), 3);
    assert!(!statement.sql.contains("Reilly") && !statement.sql.contains("123456789"));
    Ok(())
}
#[tokio::test]
async fn corrupt_oracle_backup_is_rejected_before_touching_a_real_destination_table() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let scope = backup::Scope {
        connection: "owned".into(),
        database: "main".into(),
        path: String::new(),
    };
    let path = Repository::new(root.path().into()).create_path(&scope, "corrupt.nb3")?;
    let mut writer = Writer::new(path.parent().unwrap())?;
    let mut entry = writer.json("definition.json.gz", &definition(&[column("id", "NUMBER")]))?;
    entry.checksum = "0".repeat(40);
    writer.finish(&path, &metadata(entry))?;
    sqlx::any::install_default_drivers();
    let mut connection = AnyConnection::connect("sqlite::memory:").await?;
    connection
        .execute("CREATE TABLE items(id INTEGER);INSERT INTO items VALUES(7)")
        .await?;
    let request = backup::Restore {
        scope,
        name: "corrupt".into(),
        namespace: None,
    };
    let result = restore(
        &mut connection,
        root.path(),
        &request,
        &Control::default(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("校验和"));
    let value = workbench::metadata(&mut connection, "SELECT id FROM items", &[]).await?;
    assert_eq!(workbench::text(&value[0], 0)?, "7");
    Ok(())
}
