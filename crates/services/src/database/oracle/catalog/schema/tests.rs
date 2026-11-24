//! Projection/budget checks; only the real Oracle fixture proves server grants.
use super::*;
use crate::database::Databases;
use tomato_core::Connection;

#[tokio::test]
async fn linguistic_owner_and_constraint_matches_never_attach_other_exact_identifiers() -> Result<()>
{
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    db.query(&connection, r#"
CREATE TABLE all_tab_columns(owner TEXT COLLATE NOCASE,table_name TEXT,column_name TEXT,data_type TEXT,column_id INTEGER);
CREATE TABLE all_col_comments(owner TEXT COLLATE NOCASE,table_name TEXT,column_name TEXT,comments TEXT);
CREATE TABLE all_constraints(owner TEXT COLLATE NOCASE,table_name TEXT,constraint_name TEXT,constraint_type TEXT);
CREATE TABLE all_cons_columns(owner TEXT COLLATE NOCASE,table_name TEXT,column_name TEXT,constraint_name TEXT COLLATE NOCASE);
INSERT INTO all_tab_columns VALUES('OWNER','items','first','NUMBER',1),('OWNER','items','second','NUMBER',2),('owner','items','leaked','NUMBER',3);
INSERT INTO all_col_comments VALUES('OWNER','items','first','correct'),('owner','items','first','wrong owner');
INSERT INTO all_constraints VALUES('OWNER','items','Key','P'),('OWNER','items','key','U'),('owner','items','other','P');
INSERT INTO all_cons_columns VALUES('OWNER','items','first','Key'),('OWNER','items','second','key'),('owner','items','second','Key');
"#).await?;
    let schema = project::<Column>(
        db.session(&connection).await?,
        &Scope {
            owner: "OWNER",
            own: false,
            sqlite_parameters: true,
        },
        tomato_core::limits::QUERY_BYTES,
        10_000,
    )
    .await?;
    assert_eq!(schema.tables.len(), 1);
    assert_eq!(
        schema.tables[0]
            .columns
            .iter()
            .map(|c| c.field.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(schema.tables[0].columns[0].field.comment, "correct");
    assert!(schema.tables[0].columns[0].primary);
    assert!(!schema.tables[0].columns[1].primary);
    Ok(())
}

#[tokio::test]
async fn schema_keeps_all_fields_when_case_distinct_tables_interleave_under_linguistic_sort()
-> Result<()> {
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    db.query(&connection, r#"
CREATE TABLE user_tab_columns(table_name TEXT COLLATE NOCASE,column_name TEXT,column_id INTEGER);
CREATE TABLE user_col_comments(table_name TEXT,column_name TEXT,comments TEXT);
INSERT INTO user_tab_columns VALUES('CaseOnly','upper_first',1),('CaseOnly','upper_last',2),('caseonly','lower_first',1),('caseonly','lower_last',2);
INSERT INTO user_col_comments VALUES('CaseOnly','upper_first','upper comment'),('caseonly','lower_first','lower comment');
"#).await?;
    let schema = project::<Field>(
        db.session(&connection).await?,
        &Scope {
            owner: "OWNER",
            own: true,
            sqlite_parameters: true,
        },
        tomato_core::limits::QUERY_BYTES,
        100_000,
    )
    .await?;
    assert_eq!(
        schema.tables.len(),
        2,
        "one complete field vector per exact table name is required"
    );
    for (name, expected, comment) in [
        ("CaseOnly", ["upper_first", "upper_last"], "upper comment"),
        ("caseonly", ["lower_first", "lower_last"], "lower comment"),
    ] {
        let table = schema
            .tables
            .iter()
            .find(|table| table.name == name)
            .unwrap();
        assert_eq!(
            table
                .columns
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(table.columns[0].comment, comment);
    }
    Ok(())
}

async fn fixture(db: &mut Databases, connection: &Connection) -> Result<()> {
    db.query(connection, r#"
CREATE TABLE user_tab_columns(table_name TEXT,column_name TEXT,data_type TEXT,column_id INTEGER);
CREATE TABLE user_col_comments(table_name TEXT,column_name TEXT,comments TEXT);
CREATE TABLE user_constraints(table_name TEXT,constraint_name TEXT,constraint_type TEXT);
CREATE TABLE user_cons_columns(table_name TEXT,column_name TEXT,constraint_name TEXT);
WITH RECURSIVE n(v) AS(SELECT 1 UNION ALL SELECT v+1 FROM n WHERE v<257)
INSERT INTO user_tab_columns SELECT printf('table%03d',v),'z','NUMBER',1 FROM n UNION ALL SELECT printf('table%03d',v),' key ','VARCHAR2',3 FROM n;
INSERT INTO user_constraints SELECT table_name,'pk,quote'' '||table_name,'P' FROM user_tab_columns WHERE column_name=' key ';
INSERT INTO user_cons_columns SELECT table_name,' key ',constraint_name FROM user_constraints;
INSERT INTO user_col_comments SELECT table_name,' key ','中文 '||table_name||char(10)||'备注' FROM user_constraints;
CREATE TABLE all_tab_columns AS SELECT 'OWNER' owner,* FROM user_tab_columns;
INSERT INTO all_tab_columns VALUES('OTHER','leaked','secret','NUMBER',1);
CREATE TABLE all_col_comments AS SELECT 'OWNER' owner,* FROM user_col_comments;
INSERT INTO all_col_comments VALUES('OTHER','table001',' key ','wrong owner');
CREATE TABLE all_constraints AS SELECT 'OWNER' owner,* FROM user_constraints;
CREATE TABLE all_cons_columns AS SELECT 'OWNER' owner,* FROM user_cons_columns;
"#).await?;
    Ok(())
}
#[tokio::test]
async fn schema_projections_preserve_ordinal_gaps_exact_names_owner_comments_and_primary_batches()
-> Result<()> {
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    fixture(&mut db, &connection).await?;
    let session = db.session(&connection).await?;
    for own in [true, false] {
        let scope = Scope {
            owner: "OWNER",
            own,
            sqlite_parameters: true,
        };
        let schema =
            project::<Column>(session, &scope, tomato_core::limits::QUERY_BYTES, 10_000).await?;
        assert_eq!(schema.tables.len(), 257);
        for (index, table) in schema.tables.iter().enumerate() {
            assert_eq!(table.name, format!("table{:03}", index + 1));
            assert_eq!(
                table
                    .columns
                    .iter()
                    .map(|c| c.field.name.as_str())
                    .collect::<Vec<_>>(),
                ["z", " key "]
            );
            assert!(!table.columns[0].primary);
            assert!(table.columns[1].primary);
            assert!(table.columns[0].field.comment.is_empty());
            assert_eq!(
                table.columns[1].field.comment,
                format!("中文 {}\n备注", table.name)
            );
            assert_eq!(table.columns[1].data_type, "VARCHAR2");
        }
        let fields =
            project::<Field>(session, &scope, tomato_core::limits::QUERY_BYTES, 100_000).await?;
        assert_eq!(fields.tables.len(), 257);
        assert_eq!(fields.tables[0].columns[1].comment, "中文 table001\n备注");
    }
    Ok(())
}
#[tokio::test]
async fn schema_comment_budget_failure_releases_the_cursor_and_preserves_the_session() -> Result<()>
{
    let mut db = Databases::default();
    let connection = Connection::new("SQLITE");
    fixture(&mut db, &connection).await?;
    db.query(
        &connection,
        "UPDATE user_col_comments SET comments=printf('%040000d',1)",
    )
    .await?;
    let error = project::<Field>(
        db.session(&connection).await?,
        &Scope {
            owner: "OWNER",
            own: true,
            sqlite_parameters: true,
        },
        tomato_core::limits::QUERY_BYTES,
        100_000,
    )
    .await;
    assert!(error.is_err_and(|error| error.to_string().contains("8 MiB")));
    let rows = db.query(&connection, "SELECT 42").await?;
    assert_eq!(rows[0].rows[0][0], "42");
    Ok(())
}
