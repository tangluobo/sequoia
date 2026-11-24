//! Synthetic dictionary merge checks; server grants are tested on real Oracle.
use super::*;
use crate::database::Databases;
use tomato_core::Connection;

async fn fixture(db: &mut Databases, connection: &Connection) -> Result<()> {
    db.query(connection, r#"
CREATE TABLE user_tables(table_name TEXT COLLATE NOCASE,last_analyzed TEXT,blocks INTEGER,num_rows INTEGER);
CREATE TABLE user_views(view_name TEXT COLLATE NOCASE);
CREATE TABLE user_tab_comments(table_name TEXT COLLATE NOCASE,table_type TEXT,comments TEXT);
INSERT INTO user_tables VALUES('CaseOnly','2026-11-21 01:02:03',2,7),('caseonly',NULL,NULL,NULL);
INSERT INTO user_views VALUES('view_only');
INSERT INTO user_tab_comments VALUES('CaseOnly','TABLE','upper table'),('caseonly','TABLE','lower table'),('view_only','VIEW','view comment');
CREATE TABLE all_tables(owner TEXT COLLATE NOCASE,table_name TEXT COLLATE NOCASE,last_analyzed TEXT,blocks INTEGER,num_rows INTEGER);
INSERT INTO all_tables SELECT 'OWNER',* FROM user_tables;
INSERT INTO all_tables VALUES('owner','leaked',NULL,0,0);
CREATE TABLE all_views(owner TEXT COLLATE NOCASE,view_name TEXT COLLATE NOCASE);
INSERT INTO all_views SELECT 'OWNER',* FROM user_views;
INSERT INTO all_views VALUES('owner','another_view');
CREATE TABLE all_tab_comments(owner TEXT COLLATE NOCASE,table_name TEXT COLLATE NOCASE,table_type TEXT,comments TEXT);
INSERT INTO all_tab_comments SELECT 'OWNER',* FROM user_tab_comments;
INSERT INTO all_tab_comments VALUES('owner','CaseOnly','TABLE','wrong owner');
"#).await?;
    Ok(())
}
#[tokio::test]
async fn object_headers_do_not_multiply_case_distinct_names_or_cross_attach_comments() -> Result<()>
{
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    fixture(&mut db, &connection).await?;
    let baseline = db.query(&connection, "SELECT t.table_name,c.table_name FROM user_tables t LEFT JOIN user_tab_comments c ON c.table_name=t.table_name AND c.table_type='TABLE'").await?;
    assert_eq!(baseline[0].rows.len(), 4, "old linguistic join baseline");
    for own in [true, false] {
        let (catalog, _) = project(
            db.session(&connection).await?,
            &Scope {
                owner: "OWNER",
                own,
                sqlite_parameters: true,
            },
            tomato_core::limits::QUERY_BYTES,
        )
        .await?;
        assert_eq!(catalog.objects.len(), 3);
        for (name, view, comment) in [
            ("CaseOnly", false, "upper table"),
            ("caseonly", false, "lower table"),
            ("view_only", true, "view comment"),
        ] {
            let objects: Vec<_> = catalog
                .objects
                .iter()
                .filter(|o| o.table.name == name)
                .collect();
            assert_eq!(objects.len(), 1);
            let object = objects[0];
            assert_eq!(object.table.schema, "OWNER");
            assert_eq!(object.view, view);
            assert_eq!(object.comment, comment);
            assert!(object.fields.is_empty() && object.engine.is_empty());
        }
        let upper = catalog
            .objects
            .iter()
            .find(|o| o.table.name == "CaseOnly")
            .unwrap();
        assert_eq!(upper.update_time, "2026-11-21 01:02:03");
        assert_eq!(upper.data_length, "16.00 KB");
        assert_eq!(upper.rows, "7");
        let lower = catalog
            .objects
            .iter()
            .find(|o| o.table.name == "caseonly")
            .unwrap();
        assert!(
            lower.update_time.is_empty() && lower.data_length.is_empty() && lower.rows.is_empty()
        );
        assert!(catalog.objects.last().unwrap().view, "tables precede views");
    }
    Ok(())
}
#[tokio::test]
async fn object_comment_budget_failure_releases_the_stream_and_preserves_the_session() -> Result<()>
{
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    fixture(&mut db, &connection).await?;
    db.query(
        &connection,
        "UPDATE user_tab_comments SET comments=printf('%05000000d',1)",
    )
    .await?;
    let error = project(
        db.session(&connection).await?,
        &Scope {
            owner: "OWNER",
            own: true,
            sqlite_parameters: true,
        },
        tomato_core::limits::QUERY_BYTES,
    )
    .await;
    assert!(error.is_err_and(|error| error.to_string().contains("8 MiB")));
    assert_eq!(
        db.query(&connection, "SELECT 42").await?[0].rows[0][0],
        "42"
    );
    Ok(())
}
