//! Actual owner, changed-CURRENT_SCHEMA and SELECT-only catalog/completion paths.
use super::*;
#[path = "../database_servers/objects/preload_tests.rs"]
mod preload_tests;

#[tokio::test]
#[ignore = "requires the owned Oracle loopback fixture, never saved connections"]
async fn real_oracle_schema_objects_and_completion_keep_privileges_names_comments_and_primary_keys()
-> Result<()> {
    run_owned_fixture_with(|connections, _| Box::pin(async move {
        let owner = &connections[0];
        let reader = &connections[1];
        let owner_name = owner.text("username");
        let reader_name = reader.text("username");
        let mut db = Databases::default();
        let result = async {
            let setup = r#"CREATE TABLE "a,items" ("z" NUMBER,"gone" NUMBER," same " VARCHAR2(30),"quote'name" NUMBER,CONSTRAINT "pk,quote'one" PRIMARY KEY("quote'name","z"));
ALTER TABLE "a,items" DROP COLUMN "gone";
COMMENT ON COLUMN "a,items"." same " IS '中文备注
第二行';
CREATE TABLE "a_items" (" same " VARCHAR2(30));
COMMENT ON COLUMN "a_items"." same " IS 'different table';
CREATE TABLE "secret" ("must_not_leak" NUMBER);
CREATE TABLE "CaseOnly" ("upper_first" NUMBER,"upper_last" NUMBER,CONSTRAINT "CasePk" PRIMARY KEY("upper_first"));
CREATE TABLE "caseonly" ("lower_first" NUMBER,"lower_last" NUMBER,CONSTRAINT "casepk" PRIMARY KEY("lower_last"));
COMMENT ON TABLE "CaseOnly" IS 'upper table';
COMMENT ON TABLE "caseonly" IS 'lower table';
CREATE VIEW "v,items" AS SELECT " same " AS " view ","z" FROM "a,items";
COMMENT ON COLUMN "v,items"." view " IS 'view comment'"#;
            for statement in tomato_core::sql_script::split_for("ORACLE", Some(setup)) {
                db.query(owner, statement).await.with_context(|| format!("prepare owned Oracle schema fixture: {statement}"))?;
            }
            db.query(owner, &format!(r#"GRANT SELECT ON "a,items" TO "{reader_name}";
GRANT SELECT ON "v,items" TO "{reader_name}""#)).await?;
            db.query(reader, r#"CREATE TABLE "a,items" ("reader_only" NUMBER)"#).await?;
            db.query(owner, "ALTER SESSION SET NLS_SORT=BINARY_CI; ALTER SESSION SET NLS_COMP=LINGUISTIC").await?;
            let order = db.query(owner, "SELECT table_name,column_name FROM user_tab_columns WHERE table_name IN ('CaseOnly','caseonly') ORDER BY table_name,column_id").await?;
            eprintln!("[Oracle schema] case-distinct dictionary order: {:?}", order[0].rows);
            let changed = tomato_core::database_scope::select(owner, reader_name);
            let selected_reader = tomato_core::database_scope::select(reader, owner_name);
            for (connection, schema, limited) in [(&changed, owner_name, false), (&selected_reader, owner_name, true)] {
                let current = db.query(connection, "SELECT SYS_CONTEXT('USERENV','CURRENT_SCHEMA') FROM dual").await?;
                ensure!(current[0].rows[0][0] == connection.text("__tomatoOracleOwner"), "fixture did not change CURRENT_SCHEMA");
                let started = std::time::Instant::now();
                let DatabaseOutput::Objects(catalog) = db.workbench(connection, DatabaseAction::Objects { schema: schema.into() }).await? else { anyhow::bail!("objects required") };
                preload_tests::verify(&mut db,connection,schema,&catalog).await?;
                let expected: &[&str] = &["z", " same ", "quote'name"];
                let table = catalog.objects.iter().find(|object|object.table.name=="a,items").context("table missing")?;
                ensure!(table.fields.iter().map(|field|field.name.as_str()).eq(expected.iter().copied()), "ordinal/quoted field names changed");
                ensure!(table.fields[1].comment == "中文备注\n第二行" && table.fields[0].comment.is_empty(), "field comments were changed or attached to another table");
                ensure!(catalog.objects.iter().any(|object|object.table.name=="secret") != limited, "catalog leaked or lost an owner table");
                if !limited {
                    for (name, expected) in [("CaseOnly", ["upper_first", "upper_last"]), ("caseonly", ["lower_first", "lower_last"])] {
                        ensure!(catalog.objects.iter().filter(|object| object.table.name == name).count() == 1, "linguistic join duplicated object {name}");
                        let object = catalog.objects.iter().find(|object|object.table.name==name).context("case-distinct object lost")?;
                        ensure!(object.fields.iter().map(|field|field.name.as_str()).eq(expected), "linguistic ordering lost fields for {name}");
                        ensure!(object.comment == if name == "CaseOnly" { "upper table" } else { "lower table" }, "linguistic join attached another table's comment for {name}");
                    }
                }
                let DatabaseOutput::Completion(metadata) = db.workbench(connection, DatabaseAction::Completion { schema: schema.into() }).await? else { anyhow::bail!("completion required") };
                let columns = metadata.tables.iter().find(|table|table.name=="a,items").context("completion table missing")?;
                ensure!(columns.columns.iter().map(String::as_str).eq(expected.iter().copied()), "completion column order/name differs");
                ensure!(!columns.columns.iter().any(|name|name=="reader_only"), "CURRENT_SCHEMA selected another owner's fields");
                let ai = metadata.ai_tables.iter().find(|table|table.text.starts_with("TABLE a,items\n")).context("AI schema absent")?;
                ensure!(ai.text.contains("  z NUMBER PRIMARY KEY") && ai.text.contains("  quote'name NUMBER PRIMARY KEY") && ai.text.contains("中文备注 第二行"), "type/primary/comment metadata lost: {}", ai.text);
                ensure!(metadata.tables.iter().any(|table|table.name=="secret") != limited, "completion leaked or lost an owner table");
                if !limited {
                    for (name, expected) in [("CaseOnly", ["upper_first", "upper_last"]), ("caseonly", ["lower_first", "lower_last"])] {
                        let table = metadata.tables.iter().find(|table|table.name==name).context("case-distinct completion table lost")?;
                        ensure!(table.columns.iter().map(String::as_str).eq(expected), "linguistic ordering changed completion fields for {name}");
                        let ai = metadata.ai_tables.iter().find(|table| table.text.starts_with(&format!("TABLE {name}\n"))).context("case-distinct AI schema lost")?;
                        let primary = if name == "CaseOnly" { "upper_first" } else { "lower_last" };
                        ensure!(ai.text.contains(&format!("  {primary} NUMBER PRIMARY KEY")), "linguistic constraint lookup changed primary flag for {name}");
                    }
                }
                let view = metadata.ai_tables.iter().find(|table|table.text.starts_with("TABLE v,items\n")).context("view completion missing")?;
                ensure!(!view.text.contains("PRIMARY KEY") && view.text.contains("view comment"), "view acquired base-table key flags or lost comments");
                eprintln!("[Oracle schema] owner projection limited={limited}: {:?}", started.elapsed());
            }
            let DatabaseOutput::Completion(metadata) = db.workbench(owner, DatabaseAction::Completion { schema: reader_name.into() }).await? else { anyhow::bail!("completion required") };
            ensure!(metadata.tables.is_empty(), "ungranted other-owner columns leaked");
            let DatabaseOutput::Completion(metadata) = db.workbench(&selected_reader, DatabaseAction::Completion { schema: reader_name.into() }).await? else { anyhow::bail!("completion required") };
            ensure!(metadata.tables.len()==1 && metadata.tables[0].columns==["reader_only"], "login identity must still select the reader's own catalog after CURRENT_SCHEMA changed");
            Ok(())
        }.await;
        let closed = async {
            db.disconnect(&owner.id).await?;
            db.disconnect(&reader.id).await?;
            Ok(())
        }.await;
        result.and(closed)
    })).await
}
