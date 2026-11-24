use super::*;
use std::collections::BTreeMap;
use tomato_services::database::query_rows::Change;
fn change(
    result: &tomato_services::QueryResult,
    row: usize,
    modified: impl IntoIterator<Item = (usize, Option<String>)>,
) -> Change {
    Change {
        original: (0..result.columns.len())
            .map(|column| result.value(row, column).map(str::to_owned))
            .collect(),
        modified: modified.into_iter().collect(),
        delete: false,
    }
}
async fn query_rows(kind: &str, variable: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "query editor fixture requires an owned loopback database"
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
    let table = format!("tomato_query_edit_{}", uuid::Uuid::new_v4().simple());
    let plain = format!("{table}_plain");
    let binary = if kind == "MYSQL" { "BLOB" } else { "BYTEA" };
    let bytes = if kind == "MYSQL" {
        "X'FF00FE'"
    } else {
        "decode('ff00fe','hex')"
    };
    db.query(&connection,&format!("CREATE TABLE {table}(id INTEGER PRIMARY KEY,value VARCHAR(200),amount DECIMAL(20,9),stamp DATE,payload {binary}); INSERT INTO {table} VALUES(1,'before',0.000000100,'2026-10-17',{bytes}); CREATE TABLE {plain}(value VARCHAR(200),amount DECIMAL(20,9)); INSERT INTO {plain} VALUES('Résumé',0.000000100),('Résumé',0.000000100),('resume',0.000000100)")).await?;
    let checked = async {
        let comment_sql = if kind == "MYSQL" {
            format!("ALTER TABLE {table} MODIFY COLUMN value VARCHAR(200) COMMENT '显示注释'")
        } else {
            format!("COMMENT ON COLUMN {table}.value IS '显示注释'")
        };
        db.query(&connection, &comment_sql).await?;
        let query =
            format!("SELECT 42 AS id,id AS real_id,value,amount,stamp,payload FROM {table}");
        let result = db.query(&connection, &query).await?.remove(0);
        let editing = db
            .query_editing(&connection, &table, &result)
            .await?
            .context("editable query metadata")?;
        assert_eq!(editing.plan.editable_column_indexes, [1, 2, 3, 4, 5]);
        assert!(editing.plan.can_delete());
        assert_eq!(
            editing.comments.get("value").map(String::as_str),
            Some("显示注释")
        );
        assert!(editing.plan.database_column_names[0].starts_with('\0'));
        assert_eq!(editing.plan.database_column_names[1], "id");
        let expected = "x'); DROP TABLE unrelated; -- 中文";
        assert_eq!(
            db.save_query_rows(
                &connection,
                editing.clone(),
                vec![change(
                    &result,
                    0,
                    [
                        (2, Some(expected.into())),
                        (3, Some("9.125".into())),
                        (4, Some("2026-10-18".into()))
                    ]
                )]
            )
            .await?,
            1
        );
        let after = db.query(&connection, &query).await?.remove(0);
        assert_eq!(after.value(0, 2), Some(expected));
        assert_eq!(after.value(0, 4), Some("2026-10-18"));
        let binary_sql = if kind == "MYSQL" {
            format!("SELECT HEX(payload) FROM {table}")
        } else {
            format!("SELECT upper(encode(payload,'hex')) FROM {table}")
        };
        assert_eq!(
            db.query(&connection, &binary_sql).await?[0].value(0, 0),
            Some("FF00FE"),
            "editing another cell must not rewrite a binary column through its display text"
        );
        assert!(
            db.save_query_rows(
                &connection,
                editing.clone(),
                vec![change(&after, 0, [(0, Some("99".into()))])]
            )
            .await
            .is_err(),
            "expression aliases cannot overwrite physical keys"
        );
        let mut missing = change(&after, 0, [(2, Some("must rollback".into()))]);
        missing.original[1] = Some("999".into());
        assert!(
            db.save_query_rows(
                &connection,
                editing.clone(),
                vec![
                    change(&after, 0, [(2, Some("first write".into()))]),
                    missing
                ]
            )
            .await
            .is_err()
        );
        assert_eq!(
            db.query(&connection, &query).await?[0].value(0, 2),
            Some(expected),
            "later row failure rolls back the whole batch"
        );
        db.query(&connection, "BEGIN").await?;
        let refused = db
            .save_query_rows(
                &connection,
                editing.clone(),
                vec![change(&after, 0, [(2, Some("merge denied".into()))])],
            )
            .await;
        assert!(refused.is_err());
        db.query(&connection, "ROLLBACK").await?;
        assert_eq!(
            db.query(&connection, &query).await?[0].value(0, 2),
            Some(expected)
        );
        let no_key = db
            .query(
                &connection,
                &format!("SELECT value,amount FROM {plain} WHERE value='Résumé' ORDER BY value"),
            )
            .await?
            .remove(0);
        let plan = db
            .query_editing(&connection, &plain, &no_key)
            .await?
            .context("no-key source metadata")?;
        assert!(plan.plan.uses_original_row_match);
        assert!(!plan.plan.can_delete());
        assert_eq!(
            db.save_query_rows(
                &connection,
                plan.clone(),
                vec![change(&no_key, 0, [(0, Some("changed once".into()))])]
            )
            .await?,
            1
        );
        let counts = db
            .query(
                &connection,
                &format!("SELECT COUNT(*) FROM {plain} WHERE value='changed once'"),
            )
            .await?;
        assert_eq!(
            counts[0].value(0, 0),
            Some("1"),
            "duplicate original rows update only one physical record"
        );
        let untouched = db
            .query(
                &connection,
                &format!("SELECT COUNT(*) FROM {plain} WHERE value='resume'"),
            )
            .await?;
        // MySQL's original fixture collation may ignore accents/case; the write's
        // byte-exact predicate must still leave the differently spelled row.
        assert!(untouched[0].value(0, 0).unwrap().parse::<u64>()? >= 1);
        let partial = db
            .query(&connection, &format!("SELECT value FROM {plain}"))
            .await?
            .remove(0);
        assert!(
            !db.query_editing(&connection, &plain, &partial)
                .await?
                .unwrap()
                .plan
                .is_editable()
        );
        let mut delete = change(&after, 0, BTreeMap::new());
        delete.delete = true;
        assert_eq!(
            db.save_query_rows(&connection, editing, vec![delete])
                .await?,
            1
        );
        assert!(db.query(&connection, &query).await?[0].rows.is_empty());
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let cleanup = db
        .query(
            &connection,
            &format!("DROP TABLE {plain}; DROP TABLE {table}"),
        )
        .await;
    checked?;
    cleanup?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires an owned loopback MySQL fixture"]
async fn mysql_query_rows_preserve_origins_atomicity_and_binary_values() -> Result<()> {
    query_rows("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test]
#[ignore = "requires an owned loopback PostgreSQL fixture"]
async fn postgres_query_rows_preserve_origins_atomicity_and_binary_values() -> Result<()> {
    query_rows("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
