use super::*;

async fn native_types(kind: &str, variable: &str, golden: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated fixture URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "native type fixtures require an isolated loopback database"
    );
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .map(|v| v.into_owned())
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
    let golden: serde_json::Value = serde_json::from_str(golden)?;
    let mut db = Databases::default();
    for setup in golden["setup"].as_array().context("setup required")? {
        db.query(&connection, setup.as_str().unwrap()).await?;
    }
    for case in golden["cases"].as_array().context("cases required")? {
        let sql = case["sql"].as_str().unwrap();
        let result = db.query(&connection, sql).await?;
        let result = result.first().context("query result required")?;
        let columns: Vec<_> = case["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(result.columns, columns, "{sql}");
        let rows: Vec<Vec<_>> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_array().unwrap().iter().map(|v| v.as_str()).collect())
            .collect();
        let actual: Vec<Vec<_>> = result
            .rows
            .iter()
            .enumerate()
            .map(|(row, values)| {
                (0..values.len())
                    .map(|column| result.value(row, column))
                    .collect()
            })
            .collect();
        assert_eq!(actual, rows, "{sql}");
        assert_eq!(
            serde_json::to_value(result)?["rows"],
            case["rows"],
            "JSON null semantics: {sql}"
        );
        assert!(!result.truncated);
    }
    db.disconnect(&connection.id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires dedicated loopback MySQL fixture"]
async fn mysql_native_types_match_connector_j() -> Result<()> {
    native_types(
        "MYSQL",
        "TOMATO_TEST_MYSQL_URL",
        include_str!("../fixtures/sql-types/mysql.json"),
    )
    .await
}
#[tokio::test]
#[ignore = "requires dedicated loopback PostgreSQL fixture"]
async fn postgres_native_types_match_pg_jdbc() -> Result<()> {
    native_types(
        "POSTGRESQL",
        "TOMATO_TEST_POSTGRES_URL",
        include_str!("../fixtures/sql-types/postgres.json"),
    )
    .await
}
