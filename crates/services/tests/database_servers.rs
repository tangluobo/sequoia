//! Opt-in isolated server fixtures; CI supplies dedicated, disposable databases.
use anyhow::{Context, Result};
use tomato_core::{Connection, workbench::*};
use tomato_services::database::{Databases, inserts_sql};
#[path = "database_servers/backup.rs"]
mod backup;
#[path = "database_servers/backup_postgres.rs"]
mod backup_postgres;
#[path = "database_servers/binary_columns.rs"]
mod binary_columns;
#[path = "database_servers/data_import.rs"]
mod data_import;
#[path = "database_servers/metadata.rs"]
mod metadata;
#[path = "database_servers/native_types.rs"]
mod native_types;
#[path = "database_servers/objects.rs"]
mod objects;
#[path = "database_servers/query_rows.rs"]
mod query_rows;
#[path = "database_servers/recovery.rs"]
mod recovery;
#[path = "database_servers/scopes.rs"]
mod scopes;
#[path = "database_servers/script.rs"]
mod script;
#[path = "table_filters/mod.rs"]
mod table_filters;
#[path = "database_servers/transfer.rs"]
mod transfer;

async fn fixture(kind: &str, variable: &str) -> Result<()> {
    let uri =
        url::Url::parse(&std::env::var(variable).context("missing isolated test database URL")?)?;
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .map(|s| s.into_owned())
    };
    let mut connection = Connection::new(kind);
    connection.set("host", uri.host_str().context("test host required")?);
    connection.set("port", u64::from(uri.port().context("test port required")?));
    connection.set("username", decode(uri.username())?);
    connection.set("password", decode(uri.password().unwrap_or_default())?);
    connection.set("database", uri.path().trim_start_matches('/'));
    let schema = if kind == "MYSQL" {
        connection.text("database")
    } else {
        "public"
    };
    let table = TableId {
        schema: schema.into(),
        name: format!("tomato_fixture_{}", uuid::Uuid::new_v4().simple()),
    };
    let dialect = tomato_services::dialect::resolve(kind)?;
    let qualified = format!(
        "{}.{}",
        dialect.quote(&table.schema)?,
        dialect.quote(&table.name)?
    );
    let flags = format!(
        "{}.{}",
        dialect.quote(&table.schema)?,
        dialect.quote(&(table.name.clone() + "_flags"))?
    );
    let bool_type = format!(
        "{}.{}",
        dialect.quote(&table.schema)?,
        dialect.quote(&(table.name.clone() + "_bool"))?
    );
    let mut db = Databases::default();
    table_filters::verify(&mut db, &connection, schema).await?;
    let types = if kind == "MYSQL" {
        "id BIGINT PRIMARY KEY, value DECIMAL(30,10), label TEXT, payload BLOB"
    } else {
        "id BIGINT PRIMARY KEY, value NUMERIC(30,10), label TEXT, payload BYTEA"
    };
    db.query(&connection, &format!("CREATE TABLE {qualified} ({types})"))
        .await?;
    let result: Result<()> = async {
        let request = PageRequest {
            table: table.clone(),
            offset: 0,
            limit: 2,
            sort: None,
        query: Default::default(),};
        let action = DatabaseAction::Apply {
            table: table.clone(),
            request: request.clone(),
            changes: vec![RowChange::Insert {
                values: vec![
                    Some(Cell::Text("1".into())),
                    Some(Cell::Text("12345678901234567890.1234567890".into())),
                    Some(Cell::Text("O'Reilly\\NULL中文".into())),
                    Some(Cell::Binary(vec![0, 255, 39])),
                ],
            }],
        };
        let DatabaseOutput::Applied {
            page: Some(page),
            refresh_error: None,
            ..
        } = db.workbench(&connection, action).await?
        else {
            anyhow::bail!("missing refreshed page")
        };
        assert_eq!(
            page.rows[0][1],
            Cell::Text("12345678901234567890.1234567890".into())
        );
        assert_eq!(page.rows[0][3], Cell::Binary(vec![0, 255, 39]));
        let sql = tomato_services::database::page_sql(kind, &page.definition, &request)?;
        assert!(
            !sql.to_ascii_uppercase().contains("CAST("),
            "plain executed table query: {sql}"
        );
        assert!(!sql.contains("HEX(") && !sql.contains("encode("));
        if kind == "POSTGRESQL" {
            db.query(&connection, "SET bytea_output='escape'").await?;
            let DatabaseOutput::Page(escaped) = db
                .workbench(
                    &connection,
                    DatabaseAction::Page {
                        request: request.clone(),
                    },
                )
                .await?
            else {
                anyhow::bail!("missing escaped bytea page")
            };
            assert_eq!(
                escaped.rows, page.rows,
                "native page must preserve legacy bytea output"
            );
            db.query(&connection, "SET bytea_output='hex'").await?;
            db.query(&connection, &format!("CREATE DOMAIN {bool_type} AS BOOLEAN; CREATE TABLE {flags}(enabled {bool_type},label TEXT); INSERT INTO {flags} VALUES(true,'original')")).await?;
            let flags_request = PageRequest {table:TableId{schema:table.schema.clone(),name:table.name.clone()+"_flags"},offset:0,limit:1000,sort:None,query: Default::default(),};
            let DatabaseOutput::Page(flags_page)=db.workbench(&connection,DatabaseAction::Page{request:flags_request.clone()}).await? else {anyhow::bail!("missing boolean domain page")};
            assert_eq!(flags_page.rows[0][0], Cell::Text("true".into()));
            let DatabaseOutput::Applied{page:Some(saved),refresh_error:None,..}=db.workbench(&connection,DatabaseAction::Apply{
                table:flags_request.table.clone(),request:flags_request,
                changes:vec![RowChange::Update{original:flags_page.rows[0].clone(),values:vec![Cell::Text("true".into()),Cell::Text("changed".into())]}]
            }).await? else {anyhow::bail!("boolean original-row match failed")};
            assert_eq!(saved.rows[0][1],Cell::Text("changed".into()));
        }
        let exported = inserts_sql(kind, &page.definition, &page.rows)?;
        db.query(&connection, &format!("DELETE FROM {qualified}"))
            .await?;
        db.query(&connection, &exported).await?;
        db.workbench(
            &connection,
            DatabaseAction::Transaction {
                action: TransactionAction::Begin,
            },
        )
        .await?;
        let mut values = page.rows[0].clone();
        values[2] = Cell::Text("changed".into());
        db.workbench(
            &connection,
            DatabaseAction::Apply {
                table: table.clone(),
                request: request.clone(),
                changes: vec![RowChange::Update {
                    original: page.rows[0].clone(),
                    values,
                }],
            },
        )
        .await?;
        db.workbench(
            &connection,
            DatabaseAction::Transaction {
                action: TransactionAction::Rollback,
            },
        )
        .await?;
        let DatabaseOutput::Page(after) = db
            .workbench(&connection, DatabaseAction::Page { request })
            .await?
        else {
            anyhow::bail!("missing page")
        };
        assert_eq!(
            after.rows, page.rows,
            "saving table rows must preserve outer transaction"
        );
        Ok(())
    }
    .await;
    // Only the uniquely named fixture table is removed, even after a failed assertion-free operation.
    if kind == "POSTGRESQL" {
        db.query(
            &connection,
            &format!("DROP TABLE IF EXISTS {flags}; DROP DOMAIN IF EXISTS {bool_type}"),
        )
        .await?;
    }
    let cleanup = db
        .query(&connection, &format!("DROP TABLE {qualified}"))
        .await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires TOMATO_TEST_MYSQL_URL pointing to an isolated fixture database"]
async fn mysql_workbench() -> Result<()> {
    fixture("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}

#[tokio::test]
#[ignore = "requires TOMATO_TEST_POSTGRES_URL pointing to an isolated fixture database"]
async fn postgres_workbench() -> Result<()> {
    fixture("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
