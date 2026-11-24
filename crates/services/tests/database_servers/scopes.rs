use super::*;
async fn scopes(kind: &str, variable: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated fixture URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "scope tests require a dedicated loopback fixture database"
    );
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .map(|text| text.into_owned())
    };
    let mut admin = Connection::new(kind);
    admin.set("host", "127.0.0.1");
    admin.set(
        "port",
        u64::from(uri.port().context("fixture port required")?),
    );
    admin.set("username", decode(uri.username())?);
    admin.set("password", decode(uri.password().unwrap_or_default())?);
    admin.set("database", uri.path().trim_start_matches('/'));
    let prefix = format!("tomato_scope_{}", uuid::Uuid::new_v4().simple());
    let databases = [format!("{prefix}_a"), format!("{prefix}_b")];
    let dialect = tomato_services::dialect::resolve(kind)?;
    let mut db = Databases::default();
    for name in &databases {
        db.query(&admin, &format!("CREATE DATABASE {}", dialect.quote(name)?))
            .await?;
    }
    let mut a = admin.clone();
    a.set("database", databases[0].clone());
    let mut b = admin.clone();
    b.set("database", databases[1].clone());
    let value = if kind == "MYSQL" {
        "SELECT CAST(value AS CHAR) FROM marker"
    } else {
        "SELECT value::text FROM marker"
    };
    let outcome: Result<()> = async {
        let DatabaseOutput::Databases(names) =
            db.workbench(&admin, DatabaseAction::Databases).await?
        else {
            anyhow::bail!("database catalog required")
        };
        anyhow::ensure!(
            databases.iter().all(|database| names.contains(database)),
            "actual databases must be listed independently from schemas"
        );
        for (connection, number) in [(&a, 7), (&b, 9)] {
            if kind == "POSTGRESQL" {
                db.query(connection, "CREATE SCHEMA pgodd").await?;
                let DatabaseOutput::Schemas(names) =
                    db.workbench(connection, DatabaseAction::Schemas).await?
                else {
                    anyhow::bail!("namespaces required");
                };
                anyhow::ensure!(
                    names.iter().any(|name| name == "pgodd"),
                    "pg prefix without underscore is a user schema"
                );
            }
            db.query(
                connection,
                &format!("CREATE TABLE marker(value INTEGER); INSERT INTO marker VALUES({number})"),
            )
            .await?;
        }
        db.workbench(
            &a,
            DatabaseAction::Transaction {
                action: TransactionAction::Begin,
            },
        )
        .await?;
        db.workbench(
            &b,
            DatabaseAction::Transaction {
                action: TransactionAction::Begin,
            },
        )
        .await?;
        db.query(&a, "UPDATE marker SET value=11").await?;
        db.query(&b, "UPDATE marker SET value=19").await?;
        db.workbench(
            &a,
            DatabaseAction::Transaction {
                action: TransactionAction::Rollback,
            },
        )
        .await?;
        anyhow::ensure!(
            db.query(&a, value).await?[0].rows[0][0] == "7",
            "database A rollback lost its scope"
        );
        anyhow::ensure!(
            db.query(&b, value).await?[0].rows[0][0] == "19",
            "database A rollback affected B"
        );
        db.workbench(
            &b,
            DatabaseAction::Transaction {
                action: TransactionAction::Commit,
            },
        )
        .await?;
        for (connection, database) in [(&a, &databases[0]), (&b, &databases[1])] {
            let schema = if kind == "MYSQL" {
                database.as_str()
            } else {
                "public"
            };
            let DatabaseOutput::Completion(metadata) = db
                .workbench(
                    connection,
                    DatabaseAction::Completion {
                        schema: schema.into(),
                    },
                )
                .await?
            else {
                anyhow::bail!("completion output required")
            };
            anyhow::ensure!(
                metadata.database == *database
                    && metadata
                        .tables
                        .iter()
                        .any(|table| table.name == "marker" && table.columns == ["value"]),
                "completion metadata crossed database scope"
            );
            anyhow::ensure!(
                databases
                    .iter()
                    .all(|database| metadata.databases.contains(database)),
                "database completions must contain actual databases, not namespaces"
            );
        }
        for connection in [&a, &b] {
            db.workbench(
                connection,
                DatabaseAction::Transaction {
                    action: TransactionAction::Begin,
                },
            )
            .await?;
            db.query(connection, "UPDATE marker SET value=100").await?;
        }
        db.disconnect_database(&a.id, a.text("database")).await?;
        anyhow::ensure!(
            db.query(&a, value).await?[0].rows[0][0] == "7",
            "database close retained A transaction"
        );
        anyhow::ensure!(
            db.query(&b, value).await?[0].rows[0][0] == "100",
            "database close interrupted B transaction"
        );
        db.disconnect(&admin.id).await?;
        anyhow::ensure!(
            db.query(&a, value).await?[0].rows[0][0] == "7",
            "disconnect retained A transaction"
        );
        anyhow::ensure!(
            db.query(&b, value).await?[0].rows[0][0] == "19",
            "disconnect retained B transaction"
        );
        Ok(())
    }
    .await;
    db.disconnect(&admin.id).await?;
    let mut cleanup = Databases::default();
    for name in &databases {
        cleanup
            .query(&admin, &format!("DROP DATABASE {}", dialect.quote(name)?))
            .await?;
    }
    cleanup.disconnect(&admin.id).await?;
    outcome
}
#[tokio::test]
#[ignore = "requires TOMATO_TEST_MYSQL_SCOPE_URL with CREATE DATABASE on a disposable loopback server"]
async fn mysql_database_scopes_and_disconnect() -> Result<()> {
    scopes("MYSQL", "TOMATO_TEST_MYSQL_SCOPE_URL").await
}
#[tokio::test]
#[ignore = "requires TOMATO_TEST_POSTGRES_URL for a disposable loopback server"]
async fn postgres_database_scopes_and_disconnect() -> Result<()> {
    scopes("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
