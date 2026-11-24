//! Original database/schema visibility, streamed without retaining driver rows.
use super::*;
async fn names(conn: &mut AnyConnection, sql: &str, index: usize) -> Result<Vec<String>> {
    let mut stream = sqlx::query(sql).fetch(conn);
    let mut names = Vec::new();
    let mut held = 0usize;
    while let Some(row) = stream.try_next().await? {
        if names.len() >= 10_000 {
            bail!("数据库目录超过 10,000 项，请缩小权限范围");
        }
        let name = text(&row, index)?;
        held = held.saturating_add(name.capacity());
        let before = names.capacity();
        names.push(name);
        held += (names.capacity() - before) * std::mem::size_of::<String>();
        if held > 8 * 1024 * 1024 {
            bail!("数据库目录超过 8 MiB，请缩小权限范围");
        }
    }
    Ok(names)
}
pub(in crate::database) async fn databases(
    conn: &mut AnyConnection,
    kind: &str,
) -> Result<Vec<String>> {
    let (sql, index) = match kind {
        "MYSQL" => ("SHOW DATABASES", 0),
        "HIVE" => ("SHOW DATABASES", 0),
        "POSTGRESQL" => (
            "SELECT datname::text FROM pg_database WHERE datistemplate = false AND has_database_privilege(current_user, datname, 'CONNECT') ORDER BY datname",
            0,
        ),
        "SQLITE" => ("PRAGMA database_list", 1),
        "ORACLE" => ("SELECT username FROM all_users ORDER BY username", 0),
        _ => bail!("当前数据库没有原生目录适配器"),
    };
    names(conn, sql, index).await
}
pub(super) async fn namespaces(conn: &mut AnyConnection, kind: &str) -> Result<Vec<String>> {
    if kind == "POSTGRESQL" {
        names(conn,r"SELECT schema_name::text FROM information_schema.schemata WHERE schema_name NOT LIKE 'pg\_%' AND schema_name <> 'information_schema' AND has_schema_privilege(current_user, schema_name, 'USAGE') ORDER BY schema_name",0).await
    } else {
        databases(conn, kind).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_oracle_savepoint_names_fit_older_server_identifiers() {
        for purpose in [
            "rows", "create", "design", "metadata", "objects", "admin", "query",
        ] {
            let first = savepoint_name("ORACLE", purpose);
            assert!(first.len() <= 30);
            assert!(
                first
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            );
            assert_ne!(first, savepoint_name("ORACLE", purpose));
        }
    }
    #[tokio::test]
    async fn metadata_rejects_large_payload_before_row_count_limit() -> Result<()> {
        let mut db = Databases::default();
        let connection = Connection::new("SQLITE");
        let conn = db.session(&connection).await?;
        let failure = metadata(conn,"WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<9900) SELECT printf('%01000d',x) FROM n",&[]).await;
        assert!(failure.is_err_and(|e| e.to_string().contains("8 MiB")));
        // A failed metadata capture releases its cursor and keeps the session usable.
        assert_eq!(metadata(conn, "SELECT 42", &[]).await?.len(), 1);
        Ok(())
    }
    #[tokio::test]
    async fn sqlite_catalog_keeps_quoted_database_names_and_matches_namespace_scopes() -> Result<()>
    {
        let mut db = Databases::default();
        let connection = Connection::new("SQLITE");
        db.query(
            &connection,
            "ATTACH DATABASE ':memory:' AS \"奇'库\"; CREATE TABLE \"奇'库\".items(id INTEGER)",
        )
        .await?;
        let conn = db.session(&connection).await?;
        let expected = ["main", "奇'库"];
        assert_eq!(databases(conn, "SQLITE").await?, expected);
        assert_eq!(namespaces(conn, "SQLITE").await?, expected);
        Ok(())
    }
}
