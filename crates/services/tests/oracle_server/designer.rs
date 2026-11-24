//! Exercised only by the explicitly owned Oracle server harness.
use super::*;
use tomato_core::table_design::{
    Column, Create, Dialect, Options,
    alter::{InfoPage, Snapshot},
    auxiliary,
};

async fn information(
    db: &mut Databases,
    connection: &Connection,
    table: &TableId,
    page: InfoPage,
) -> Result<tomato_core::table_design::alter::InfoTable> {
    let DatabaseOutput::DesignInfo { info, .. } = db
        .workbench(
            connection,
            DatabaseAction::DesignInfo {
                table: table.clone(),
                page,
            },
        )
        .await?
    else {
        anyhow::bail!("expected designer information")
    };
    Ok(info)
}

fn definition(owner: &str, name: &str) -> Create {
    Create {
        table: TableId { schema: owner.into(), name: name.into() },
        columns: vec![
            Column { name: "id".into(), data_type: "NUMBER".into(), primary_key: true, not_null: true, ..Default::default() },
            Column { name: "label".into(), data_type: "VARCHAR2".into(), length: "100".into(), ..Default::default() },
        ],
        indexes: vec![], options: Options::default(), comment: "trigger preview roundtrip".into(),
        auxiliary: vec![auxiliary::Edit {
            before_signature: String::new(), page: InfoPage::Triggers, before: vec![],
            after: vec![
                vec![format!("{name}_ins"), "BEFORE".into(), "INSERT".into(), "BEGIN :NEW.\"label\" := 'insert effect'; END;".into()],
                vec![format!("{name}_upd"), "BEFORE".into(), "UPDATE".into(), "DECLARE n NUMBER := 1; BEGIN :NEW.\"label\" := 'update effect'; END;\n-- retain trailing comment".into()],
            ],
        }],
    }
}

#[tokio::test]
#[ignore = "requires the owned Oracle server fixture, never saved connections"]
async fn real_oracle_designer_triggers_preserve_plsql_and_copyable_preview() -> Result<()> {
    super::run_owned_fixture_with(|connections, admin| Box::pin(async move {
        let connection = &connections[0];
        let owner = connection.text("username");
        let mut db = Databases::default();
        foreign_keys(&mut db, &connections[0], &connections[1]).await?;
        for (name, copy_preview) in [("actor_items", false), ("preview_items", true)] {
            let request = definition(owner, name);
            let table = request.table.clone();
            if copy_preview {
                let sql = request.sql(Dialect::Oracle)?;
                ensure!(sql.matches("\n/").count()==2 && !sql.contains("END;;"), "preview must delimit both PL/SQL units");
                db.query(connection, &sql).await?;
            } else {
                db.workbench(connection, DatabaseAction::CreateTable { request }).await?;
            }
            let target = format!("\"{owner}\".\"{name}\"");
            db.query(connection, &format!("INSERT INTO {target}(\"id\",\"label\") VALUES(1,'before')")).await?;
            let inserted = db.query(connection, &format!("SELECT \"label\" FROM {target}")).await?;
            ensure!(inserted[0].rows[0][0]=="insert effect", "saved INSERT trigger did not fire");
            db.query(connection, &format!("UPDATE {target} SET \"label\"='before'")).await?;
            let updated = db.query(connection, &format!("SELECT \"label\" FROM {target}")).await?;
            ensure!(updated[0].rows[0][0]=="update effect", "saved UPDATE trigger did not fire");
            let info = information(&mut db, connection, &table, InfoPage::Triggers).await?;
            ensure!(info.rows.len()==2, "both trigger definitions must be readable");
            let DatabaseOutput::Design(snapshot) = db.workbench(connection, DatabaseAction::Design { table: table.clone() }).await? else {
                anyhow::bail!("expected designer snapshot")
            };
            let Snapshot { definition: mut after, .. } = (*snapshot).clone();
            after.auxiliary.push(auxiliary::Edit { page: InfoPage::Triggers, before_signature: info.signature, before: info.rows, after: vec![] });
            let origins = after.columns.iter().map(|column| Some(column.name.clone())).collect();
            db.workbench(connection, DatabaseAction::SaveDesign { edit: Box::new(tomato_core::table_design::alter::Edit { before: snapshot.baseline(), after, origins }) }).await?;
            ensure!(information(&mut db, connection, &table, InfoPage::Triggers).await?.rows.is_empty(), "saved trigger deletion must be visible");
            db.query(connection, &format!("UPDATE {target} SET \"label\"='after deletion'")).await?;
            let rows=db.query(connection, &format!("SELECT \"label\" FROM {target}")).await?;
            ensure!(rows[0].rows[0][0]=="after deletion", "deleted trigger must no longer run");
        }
        db.query(connection, r#"CREATE VIEW "editable_view" AS SELECT "id","label" FROM "actor_items";
CREATE TRIGGER "view_insert" INSTEAD OF INSERT ON "editable_view" FOR EACH ROW BEGIN INSERT INTO "actor_items" ("id","label") VALUES(:NEW."id",:NEW."label"); END;
/"#).await?;
        let view = TableId { schema: owner.into(), name: "editable_view".into() };
        let info = information(&mut db, connection, &view, InfoPage::Triggers).await?;
        ensure!(info.rows.len()==1 && info.rows[0][1]=="INSTEAD OF", "view trigger timing must be preserved");
        db.query(connection, r#"INSERT INTO "editable_view" ("id","label") VALUES(2,'view effect')"#).await?;
        let rows = db.query(connection, r#"SELECT "label" FROM "actor_items" WHERE "id"=2"#).await?;
        ensure!(rows[0].rows[0][0]=="view effect", "INSTEAD OF trigger must insert into the underlying table");
        db.disconnect(&connection.id).await?;
        for connection in connections {
            let username = connection.text("username");
            let sessions = db.query(admin, &format!("SELECT COUNT(*) FROM v$session WHERE username='{username}'")).await?;
            ensure!(sessions[0].rows[0][0] == "0", "disconnect acknowledged before the owned Oracle session ended");
        }
        db.disconnect(&admin.id).await?;
        Ok(())
    })).await
}

async fn foreign_keys(db: &mut Databases, parent: &Connection, child: &Connection) -> Result<()> {
    let parent_owner = parent.text("username");
    let child_owner = child.text("username");
    eprintln!("[oracle designer] scoped composite foreign keys and quoted names");
    db.query(parent, &format!(r#"CREATE TABLE "parent,ui" ("comma,id" NUMBER," space " NUMBER,PRIMARY KEY(" space ","comma,id"));
GRANT REFERENCES ON "parent,ui" TO "{child_owner}";
INSERT INTO "parent,ui" VALUES(7,3)"#)).await?;
    db.query(child, &format!(r#"CREATE TABLE "child,ui" ("left,id" NUMBER," right " NUMBER,CONSTRAINT "fk,ui" FOREIGN KEY("left,id"," right ") REFERENCES "{parent_owner}"."parent,ui"(" space ","comma,id") ON DELETE CASCADE);
GRANT SELECT ON "child,ui" TO "{parent_owner}";
INSERT INTO "child,ui" VALUES(3,7)"#)).await?;
    let table = TableId {
        schema: child_owner.into(),
        name: "child,ui".into(),
    };
    let expected = vec![
        "fk,ui".into(),
        "\"left,id\",\" right \"".into(),
        parent_owner.into(),
        "parent,ui".into(),
        "\" space \",\"comma,id\"".into(),
        "CASCADE".into(),
        "NO ACTION".into(),
    ];
    let scoped = tomato_core::database_scope::select(child, parent_owner);
    let current = db
        .query(
            &scoped,
            "SELECT SYS_CONTEXT('USERENV','CURRENT_SCHEMA') FROM dual",
        )
        .await?;
    ensure!(
        current[0].rows[0][0] == parent_owner,
        "scoped fixture must change current schema"
    );
    for connection in [&scoped, parent] {
        let info = information(db, connection, &table, InfoPage::ForeignKeys).await?;
        ensure!(
            info.rows == [expected.clone()],
            "foreign-key columns/order/quoting or cross-owner reference differs: {:?}",
            info.rows
        );
        let DatabaseOutput::Objects(catalog) = db
            .workbench(
                connection,
                DatabaseAction::Objects {
                    schema: child_owner.into(),
                },
            )
            .await?
        else {
            anyhow::bail!("expected object catalog");
        };
        let key = catalog
            .foreign_keys
            .iter()
            .find(|key| key.name == "fk,ui")
            .context("catalog lost foreign key")?;
        ensure!(
            key.columns == ["left,id", " right "]
                && key.referenced_columns == [" space ", "comma,id"]
                && key.referenced.schema == parent_owner
                && key.referenced.name == "parent,ui",
            "object catalog must retain exact component names and positions"
        );
    }
    let mut row = expected;
    row[0] = "fk_copy".into();
    let copy = Create {
        table: TableId {
            schema: child_owner.into(),
            name: "child copied".into(),
        },
        columns: ["left,id", " right "]
            .map(|name| Column {
                name: name.into(),
                data_type: "NUMBER".into(),
                ..Default::default()
            })
            .to_vec(),
        auxiliary: vec![auxiliary::Edit {
            before_signature: String::new(),
            page: InfoPage::ForeignKeys,
            before: vec![],
            after: vec![row],
        }],
        indexes: vec![],
        options: Options::default(),
        comment: String::new(),
    };
    db.query(child, &copy.sql(Dialect::Oracle)?).await?;
    db.query(child, r#"INSERT INTO "child copied" VALUES(3,7)"#)
        .await?;
    ensure!(
        db.query(child, r#"INSERT INTO "child copied" VALUES(8,9)"#)
            .await
            .is_err(),
        "copied foreign key must enforce the relationship"
    );
    db.query(parent, r#"DELETE FROM "parent,ui""#).await?;
    for name in ["child,ui", "child copied"] {
        let rows = db
            .query(child, &format!("SELECT COUNT(*) FROM \"{name}\""))
            .await?;
        ensure!(
            rows[0].rows[0][0] == "0",
            "copied CASCADE rule must delete matching rows"
        );
        db.query(child, &format!("DROP TABLE \"{name}\" PURGE"))
            .await?;
    }
    db.query(parent, r#"DROP TABLE "parent,ui" PURGE"#).await?;
    db.disconnect(&child.id).await?;
    Ok(())
}
