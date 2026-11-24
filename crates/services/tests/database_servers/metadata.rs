use super::*;
async fn metadata(kind: &str, variable: &str, golden_kind: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated fixture URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "metadata tests require a dedicated loopback database"
    );
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .map(|text| text.into_owned())
    };
    let mut connection = Connection::new(kind);
    connection.set("host", "127.0.0.1");
    connection.set(
        "port",
        u64::from(uri.port().context("fixture port required")?),
    );
    connection.set("username", decode(uri.username())?);
    connection.set("password", decode(uri.password().unwrap_or_default())?);
    connection.set("database", uri.path().trim_start_matches('/'));
    let mut db = Databases::default();
    db.query(&connection, "DROP TABLE IF EXISTS tomato_origin_fixture")
        .await?;
    db.query(&connection,&format!("CREATE TABLE tomato_origin_fixture(id INTEGER PRIMARY KEY,label VARCHAR(30),stamp DATE,payload {})",if kind=="MYSQL" {"BLOB"} else {"BYTEA"})).await?;
    db.query(&connection,"INSERT INTO tomato_origin_fixture(id,label,stamp) VALUES(1,'中文','2024-02-29'),(2,NULL,NULL)").await?;
    db.query(
        &connection,
        if kind == "MYSQL" {
            "ALTER TABLE tomato_origin_fixture MODIFY label VARCHAR(30) COMMENT '邮箱\n备注'"
        } else {
            "COMMENT ON COLUMN tomato_origin_fixture.label IS '邮箱\n备注'"
        },
    )
    .await?;
    let schema = if kind == "MYSQL" {
        connection.text("database")
    } else {
        "public"
    };
    let DatabaseOutput::Completion(completion) = db
        .workbench(
            &connection,
            DatabaseAction::Completion {
                schema: schema.into(),
            },
        )
        .await?
    else {
        anyhow::bail!("completion required");
    };
    let structure = completion
        .ai_tables
        .iter()
        .find(|table| table.text.starts_with("TABLE tomato_origin_fixture\n"))
        .context("AI schema fixture absent")?;
    assert!(
        structure.text.contains(if kind == "MYSQL" {
            "  id INT PRIMARY KEY"
        } else {
            "  id int4 PRIMARY KEY"
        }),
        "{}",
        structure.text
    );
    assert!(
        structure.text.contains(if kind == "MYSQL" {
            "  label VARCHAR -- 邮箱 备注"
        } else {
            "  label varchar -- 邮箱 备注"
        }),
        "{}",
        structure.text
    );
    assert!(structure.terms.iter().any(|term| term == "邮箱\n备注"));
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/sql-types/metadata.json"))?;
    for case in golden[golden_kind]
        .as_array()
        .context("metadata cases required")?
    {
        let sql = case["sql"].as_str().unwrap();
        let results = db.query(&connection, sql).await?;
        let result = results.first().context("result set required")?;
        assert!(result.is_rowset(), "{sql}");
        assert!(result.origin_error.is_none(), "{:?}", result.origin_error);
        assert_eq!(
            serde_json::to_value(&result.columns)?,
            case["labels"],
            "{sql}"
        );
        assert_eq!(
            serde_json::to_value(&result.column_types)?,
            case["types"],
            "{sql}"
        );
        for (name, actual) in [
            (
                "columns",
                result
                    .origins
                    .iter()
                    .map(|origin| origin.column.as_str())
                    .collect::<Vec<_>>(),
            ),
            (
                "tables",
                result
                    .origins
                    .iter()
                    .map(|origin| origin.table.as_str())
                    .collect(),
            ),
            (
                "schemas",
                result
                    .origins
                    .iter()
                    .map(|origin| origin.schema.as_str())
                    .collect(),
            ),
            (
                "catalogs",
                result
                    .origins
                    .iter()
                    .map(|origin| origin.catalog.as_str())
                    .collect(),
            ),
        ] {
            assert_eq!(serde_json::to_value(actual)?, case[name], "{name}: {sql}");
        }
        let rows: Vec<Vec<_>> = result
            .rows
            .iter()
            .enumerate()
            .map(|(row, values)| {
                (0..values.len())
                    .map(|column| result.value(row, column))
                    .collect()
            })
            .collect();
        assert_eq!(serde_json::to_value(rows)?, case["rows"], "{sql}");
        assert_eq!(
            result.row_count(),
            case["rows"].as_array().unwrap().len() as u64
        );
    }
    db.query(&connection, "DROP TABLE tomato_origin_fixture")
        .await?;
    db.disconnect(&connection.id).await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires dedicated loopback MySQL fixture"]
async fn mysql_metadata_matches_original_service() -> Result<()> {
    metadata("MYSQL", "TOMATO_TEST_MYSQL_URL", "mysql").await
}
#[tokio::test]
#[ignore = "requires dedicated loopback PostgreSQL fixture"]
async fn postgres_metadata_matches_original_service() -> Result<()> {
    metadata("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL", "postgres").await
}
