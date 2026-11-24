use super::*;
use tomato_services::database::script::Mode;
async fn script(kind: &str, variable: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "script fixture requires owned loopback database"
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
    let name = format!("tomato_script_{}", uuid::Uuid::new_v4().simple());
    db.query(
        &connection,
        &format!("CREATE TABLE {name}(id INTEGER PRIMARY KEY, value VARCHAR(32))"),
    )
    .await?;
    let checked=async {
        let sql=format!("INSERT INTO {name} VALUES(1,'semi;colon'); SELECT absent_column FROM {name}; SELECT id AS key_alias, value, NULL AS missing FROM {name}; SELECT id, value FROM {name} WHERE id=99; UPDATE {name} SET value='after error' WHERE id=1");
        let script=db.script(&connection,sql.clone(),Mode::Run).await?;
        assert_eq!((script.success_count,script.fail_count),(4,1));assert_eq!(script.statements.len(),5);
        assert!(!script.statements[1].success);assert!(script.statements[4].success);
        let first=&script.results[script.statements[2].result.unwrap()];
        assert_eq!(first.columns,["key_alias","value","missing"]);assert_eq!(first.value(0,1),Some("semi;colon"));assert_eq!(first.value(0,2),None);
        assert_eq!(first.origins[0].column,"id");assert_eq!(first.origins[0].table,name);
        let empty=&script.results[script.statements[3].result.unwrap()];assert!(empty.is_rowset());assert_eq!(empty.columns,["id","value"]);assert_eq!(empty.row_count(),0);
        assert_eq!(script.plans.len(),2);assert!(script.status.is_some());
        assert!(script.info().ends_with("成功: 4  失败: 1"));
        let explain=db.script(&connection,format!("DELETE FROM {name}; SELECT id FROM {name}"),Mode::Explain).await?;
        assert_eq!(explain.plans.len(),1);assert!(explain.statements.is_empty());assert!(explain.status.is_none());
        let retained=db.query(&connection,&format!("SELECT value FROM {name} WHERE id=1")).await?;
        assert_eq!(retained[0].value(0,0),Some("after error"));
        let transaction=db.script(&connection,format!("BEGIN; UPDATE {name} SET value='rollback' WHERE id=1; SELECT value FROM {name}; ROLLBACK; SELECT value FROM {name}"),Mode::Run).await?;
        assert_eq!(transaction.fail_count,0);
        assert_eq!(transaction.results[transaction.statements[2].result.unwrap()].value(0,0),Some("rollback"));
        assert_eq!(transaction.results[transaction.statements[4].result.unwrap()].value(0,0),Some("after error"));
        Ok::<_,anyhow::Error>(())
    }.await;
    let cleanup = db.query(&connection, &format!("DROP TABLE {name}")).await;
    checked?;
    cleanup?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires an owned loopback MySQL fixture"]
async fn mysql_sql_script_continues_and_explains_without_replaying_writes() -> Result<()> {
    script("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test]
#[ignore = "requires an owned loopback PostgreSQL fixture"]
async fn postgres_sql_script_continues_and_explains_without_replaying_writes() -> Result<()> {
    script("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
