//! Opt-in real Oracle fixture. Never loads saved connections or existing schemas.
use anyhow::{Context as _, Result, ensure};
use tomato_core::{Connection, backup, table_transfer, workbench::*};
use tomato_services::{Databases, backup::Control, database::transfer};
#[path = "oracle_server/designer.rs"]
mod oracle_designer;
#[path = "oracle_server/schema.rs"]
mod oracle_schema;

async fn page(db: &mut Databases, connection: &Connection, table: &TableId) -> Result<TablePage> {
    let DatabaseOutput::Page(page) = db
        .workbench(
            connection,
            DatabaseAction::Page {
                request: request(table),
            },
        )
        .await?
    else {
        anyhow::bail!("expected table page")
    };
    Ok(page)
}
fn request(table: &TableId) -> PageRequest {
    PageRequest {
        table: table.clone(),
        offset: 0,
        limit: 20,
        sort: None,
        query: Default::default(),
    }
}
async fn verify(
    source: &Connection,
    target: &Connection,
    admin: &Connection,
    catalog_only: bool,
) -> Result<()> {
    let mut db = Databases::default();
    let owner = source.text("username");
    let table = TableId {
        schema: owner.into(),
        name: "items".into(),
    };
    let before = serde_json::to_value(source)?;
    eprintln!("[oracle fixture] create tables, comments, function, view and trigger");
    db.query(source,r#"CREATE TABLE "items" ("id" NUMBER GENERATED ALWAYS AS IDENTITY (START WITH 37 INCREMENT BY 7) PRIMARY KEY,"label" CLOB,"payload" BLOB,"amount" NUMBER(38,18),"moment" TIMESTAMP(9) WITH TIME ZONE,"missing" VARCHAR2(20));
CREATE TABLE "child" ("id" NUMBER PRIMARY KEY,"parent" NUMBER,CONSTRAINT "parent_fk" FOREIGN KEY ("parent") REFERENCES "items"("id"));
CREATE INDEX "amount_idx" ON "items"("amount");
COMMENT ON TABLE "items" IS '中文表';
COMMENT ON COLUMN "items"."label" IS '内容备注';
CREATE OR REPLACE TRIGGER "keep_values" BEFORE INSERT ON "items" FOR EACH ROW BEGIN NULL; END;
/
CREATE OR REPLACE FUNCTION "one" RETURN NUMBER AS BEGIN RETURN 1; END;
/
CREATE VIEW "summary" AS SELECT "id","amount" FROM "items";"#).await?;
    let binary: Vec<u8> = (0..5001).map(|n| (n % 251) as u8).collect();
    let values = vec![
        None,
        Some(Cell::Text("中文 O'Reilly\u{1e}\nC:\\".repeat(1500))),
        Some(Cell::Binary(binary.clone())),
        Some(Cell::Text("12345678901234567890.123456789012345678".into())),
        Some(Cell::Text("2024-02-29 12:34:56.123456789 +08:00".into())),
        Some(Cell::Null),
    ];
    eprintln!("[oracle fixture] insert and read back native LOB/identity/precision values");
    db.workbench(
        source,
        DatabaseAction::Apply {
            table: table.clone(),
            request: request(&table),
            changes: vec![
                RowChange::Insert { values },
                RowChange::Insert {
                    values: vec![
                        None,
                        Some(Cell::Text(String::new())),
                        Some(Cell::Binary(vec![])),
                        Some(Cell::Null),
                        Some(Cell::Null),
                        Some(Cell::Text("empty LOB".into())),
                    ],
                },
            ],
        },
    )
    .await?;
    let original = page(&mut db, source, &table).await?;
    ensure!(
        original.total == 2 && original.rows[0][2] == Cell::Binary(binary),
        "Oracle native large BLOB roundtrip"
    );
    ensure!(
        original.rows[1][1] == Cell::Text(String::new())
            && original.rows[1][2] == Cell::Binary(vec![]),
        "empty LOB must remain distinct from NULL"
    );
    ensure!(
        original.rows[0][4] == Cell::Text("2024-02-29 12:34:56.123456789 +08:00".into()),
        "Oracle timestamp must preserve its local date, nanoseconds and offset"
    );
    db.query(source, r#"INSERT INTO "child" VALUES(1,37)"#)
        .await?;
    if catalog_only {
        return verify_catalog_transfer(&mut db, source, target, admin, &table, &original).await;
    }
    let transfer = table_transfer::Request {
        source: source.clone(),
        target: target.clone(),
        source_schema: owner.into(),
        target_schema: target.text("username").into(),
        tables: vec!["items".into()],
        structure: true,
        data: true,
        drop_existing: false,
        add: true,
        drop: false,
        modify: true,
    };
    eprintln!("[oracle fixture] preview and execute table copy");
    let plan = transfer::preview(&transfer, &Control::default()).await?;
    ensure!(
        plan.tables.len() == 1 && !plan.tables[0].sql.is_empty(),
        "actual Oracle copy preview"
    );
    let copied = transfer::execute(&transfer, &plan, &Control::default()).await?;
    ensure!(
        copied.failed.is_empty() && copied.records == 2,
        "Oracle copy/transfer failed: {:?}",
        copied.failed
    );
    let destination = TableId {
        schema: target.text("username").into(),
        name: "items".into(),
    };
    ensure!(
        page(&mut db, target, &destination).await?.rows == original.rows,
        "copied identity/precision/LOB/timestamp values differ"
    );
    let identities = db
        .query(
            target,
            "SELECT generation_type FROM user_tab_identity_cols WHERE table_name='items'",
        )
        .await?;
    ensure!(
        identities[0].rows[0][0] == "ALWAYS",
        "copy did not restore original identity mode"
    );
    db.query(
        target,
        r#"INSERT INTO "items"("label") VALUES('after copy')"#,
    )
    .await?;
    let next = db.query(target, r#"SELECT MAX("id") FROM "items""#).await?;
    ensure!(
        next[0].rows[0][0].parse::<i64>()? > 44,
        "copied identity sequence was not advanced"
    );
    eprintln!("[oracle fixture] enumerate backup objects and create archive");
    let objects = db.backup_objects(source, owner).await?;
    ensure!(
        objects.len() == 4,
        "expected two tables, view and function in real Oracle catalog"
    );
    let directory = tempfile::tempdir()?;
    let scope = backup::Scope {
        connection: source.name.clone(),
        database: owner.into(),
        path: String::new(),
    };
    let created = db
        .backup(
            source,
            directory.path().into(),
            backup::Operation::Create(backup::Create {
                scope: scope.clone(),
                filename: "native.nb3".into(),
                objects,
                comment: "Oracle native fixture".into(),
                lock_tables: false,
                single_transaction: true,
            }),
            None,
        )
        .await?;
    ensure!(
        created.records == 3 && created.objects == 4,
        "Oracle backup counts"
    );
    db.query(source,r#"DROP VIEW "summary"; DROP FUNCTION "one"; DROP TABLE "child" PURGE; DROP TABLE "items" PURGE"#).await?;
    eprintln!("[oracle fixture] restore and verify dependencies, identity and exact rows");
    let restored =
        restore_archive(&mut db, source, directory.path().into(), scope, "native").await?;
    ensure!(restored.records == 3, "Oracle phased restore record count");
    verify_rows(page(&mut db, source, &table).await?.rows, original.rows)?;
    let status = db
        .query(
            source,
            "SELECT object_type,object_name FROM user_objects WHERE status<>'VALID'",
        )
        .await?;
    let errors = db
        .query(source,"SELECT type,name,line,position,text FROM user_errors WHERE attribute='ERROR' ORDER BY name,sequence")
        .await?;
    ensure!(
        status[0].rows.is_empty() && errors[0].rows.is_empty(),
        "restored invalid objects {:?}; Oracle diagnostics {:?}",
        status[0].rows,
        errors[0].rows,
    );
    let triggers = db
        .query(
            source,
            "SELECT status FROM user_triggers WHERE trigger_name='keep_values'",
        )
        .await?;
    ensure!(
        triggers[0].rows == [["ENABLED"]],
        "restored trigger must be enabled"
    );
    let constraints=db.query(source,"SELECT constraint_name FROM user_constraints WHERE table_name='child' AND constraint_type='R'").await?;
    ensure!(
        constraints[0].rows.len() == 1,
        "restore lost the deferred foreign key"
    );
    let comment = db
        .query(
            source,
            "SELECT comments FROM user_tab_comments WHERE table_name='items'",
        )
        .await?;
    ensure!(
        comment[0].rows[0][0] == "中文表",
        "restore lost table comment"
    );
    db.query(
        source,
        r#"INSERT INTO "items"("label") VALUES('after restore')"#,
    )
    .await?;
    let next = db.query(source, r#"SELECT MAX("id") FROM "items""#).await?;
    ensure!(
        next[0].rows[0][0].parse::<i64>()? > 44,
        "restored identity sequence was not advanced"
    );
    ensure!(
        serde_json::to_value(source)? == before,
        "original service/owner/unknown connection fields changed"
    );
    eprintln!(
        "[oracle fixture] execute a 35-second unit, then recover our own killed idle session"
    );
    db.query(source, "BEGIN DBMS_SESSION.SLEEP(35); END;")
        .await?;
    let before_loss = page(&mut db, source, &table).await?;
    let ids = db
        .query(source, "SELECT SYS_CONTEXT('USERENV','SID') FROM dual")
        .await?;
    let sid = ids[0].rows[0][0].parse::<u32>()?;
    ensure!(
        owner.starts_with("TMT_S_")
            && owner
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "Only the owned fixture session may be interrupted"
    );
    let sessions = db
        .query(
            admin,
            &format!("SELECT serial# FROM v$session WHERE sid={sid} AND username='{owner}'"),
        )
        .await?;
    ensure!(
        sessions[0].rows.len() == 1,
        "Owned source session is not unique"
    );
    let serial = sessions[0].rows[0][0].parse::<u32>()?;
    if let Err(error) = db
        .query(
            admin,
            &format!("ALTER SYSTEM KILL SESSION '{sid},{serial}' IMMEDIATE"),
        )
        .await
    {
        // Oracle may acknowledge an asynchronous kill with ORA-00031. The
        // owned SID/serial was verified above; the actual reconnect/read below
        // must still succeed. Every other failure remains a test failure.
        ensure!(
            format!("{error:#}").contains("ORA-00031"),
            "Owned session kill failed: {error:#}"
        );
    }
    ensure!(
        page(&mut db, source, &table).await?.rows == before_loss.rows,
        "Read-only page failed to reconnect after our own idle session was killed"
    );
    db.disconnect(&source.id).await?;
    db.disconnect(&target.id).await?;
    Ok(())
}

async fn verify_catalog_transfer(
    db: &mut Databases,
    source: &Connection,
    target: &Connection,
    admin: &Connection,
    table: &TableId,
    original: &TablePage,
) -> Result<()> {
    db.query(
        source,
        r#"CREATE TABLE "composite_key" ("a" NUMBER, "b" NUMBER, PRIMARY KEY ("b","a"))"#,
    )
    .await?;
    let key = TableId {
        schema: table.schema.clone(),
        name: "composite_key".into(),
    };
    let columns = page(db, source, &key).await?.definition.columns;
    ensure!(
        columns
            .iter()
            .map(|column| (column.name.as_str(), column.primary_key))
            .eq([("a", 2), ("b", 1)]),
        "Oracle composite primary key positions must follow the constraint"
    );
    let destination = TableId {
        schema: target.text("username").into(),
        name: table.name.clone(),
    };
    let mut request = table_transfer::Request {
        source: tomato_core::database_scope::select(admin, &table.schema),
        target: tomato_core::database_scope::select(admin, &destination.schema),
        source_schema: table.schema.clone(),
        target_schema: destination.schema.clone(),
        tables: vec![table.name.clone()],
        structure: true,
        data: true,
        drop_existing: false,
        add: true,
        drop: false,
        modify: true,
    };
    eprintln!("[oracle fixture] same-connection CTAS and exact row copy");
    let control = Control::default();
    let plan = transfer::preview(&request, &control).await?;
    ensure!(
        plan.tables[0].sql.len() == 1
            && plan.tables[0].sql[0].contains(" AS SELECT * FROM ")
            && plan.tables[0].sql[0].ends_with(" WHERE 1=0"),
        "same-connection Oracle copy should use the Java CTAS statement"
    );
    let expected_insert = format!(
        "INSERT INTO \"{}\".\"items\" SELECT * FROM \"{}\".\"items\"",
        destination.schema, table.schema
    );
    ensure!(plan.tables[0].data_sql.as_deref() == Some(expected_insert.as_str()));
    ensure!(plan.preview().contains(&expected_insert));
    eprintln!("[oracle fixture] CTAS preview confirmed; revalidate and execute");
    let copied = transfer::execute(&request, &plan, &control).await?;
    ensure!(
        copied.failed.is_empty() && copied.records == 2,
        "same-connection copy failed: {:?}",
        copied.failed
    );
    eprintln!("[oracle fixture] read back the CTAS destination");
    compare_catalog_rows(original, page(db, target, &destination).await?)?;

    eprintln!("[oracle fixture] separate-session data-only copy and stale preview rejection");
    db.query(target, r#"TRUNCATE TABLE "items""#).await?;
    request.source = source.clone();
    request.target = target.clone();
    request.structure = false;
    let plan = transfer::preview(&request, &Control::default()).await?;
    ensure!(
        plan.tables[0].sql.is_empty() && plan.tables[0].data && plan.tables[0].data_sql.is_none()
    );
    db.query(target, r#"ALTER TABLE "items" ADD ("extra" VARCHAR2(20))"#)
        .await?;
    let stale = transfer::execute(&request, &plan, &Control::default())
        .await
        .unwrap_err();
    ensure!(stale.to_string().contains("结构已变化"));
    ensure!(page(db, target, &destination).await?.total == 0);
    db.query(target, r#"ALTER TABLE "items" DROP COLUMN "extra""#)
        .await?;
    let plan = transfer::preview(&request, &Control::default()).await?;
    let copied = transfer::execute(&request, &plan, &Control::default()).await?;
    ensure!(copied.failed.is_empty() && copied.records == 2);
    compare_catalog_rows(original, page(db, target, &destination).await?)?;
    db.disconnect(&source.id).await?;
    db.disconnect(&target.id).await?;
    Ok(())
}

fn compare_catalog_rows(expected: &TablePage, mut actual: TablePage) -> Result<()> {
    // The source has a primary key while the Java-style CTAS destination has
    // no implicit ordering. Match by fixture ID and compare every field.
    actual.rows.sort_by_cached_key(|row| row[0].display());
    let mut expected_rows = expected.rows.iter().collect::<Vec<_>>();
    expected_rows.sort_by_cached_key(|row| row[0].display());
    ensure!(
        expected_rows.len() == actual.rows.len(),
        "Copied row count differs"
    );
    for (expected_row, actual_row) in expected_rows.into_iter().zip(&actual.rows) {
        ensure!(
            expected_row.len() == actual_row.len(),
            "Copied column count differs"
        );
        for (i, (expected, actual)) in expected_row.iter().zip(actual_row).enumerate() {
            let value = |cell: &Cell| match cell {
                Cell::Text(text) if text.len() > 120 => format!("text {} bytes", text.len()),
                Cell::Binary(data) => format!("binary {} bytes", data.len()),
                _ => format!("{cell:?}"),
            };
            ensure!(
                expected == actual,
                "Copied Oracle column {} differs: expected {}, got {}",
                i + 1,
                value(expected),
                value(actual)
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "Requires an owned disposable loopback Oracle Free/XE container; never uses saved connections"]
async fn real_oracle_native_lobs_identity_copy_nb3_dependencies_and_restore() -> Result<()> {
    run_owned_fixture(false).await
}

#[tokio::test]
#[ignore = "Requires an owned disposable loopback Oracle Free/XE container; never uses saved connections"]
async fn real_oracle_same_connection_and_data_only_transfer() -> Result<()> {
    run_owned_fixture(true).await
}

#[tokio::test]
#[ignore = "Requires an owned disposable loopback Oracle Free/XE container"]
async fn real_oracle_primary_key_catalog_respects_the_authenticated_owner() -> Result<()> {
    run_owned_fixture_with(|connections, _| {
        Box::pin(verify_primary_key_catalog(&connections[0], &connections[1]))
    })
    .await
}

async fn verify_primary_key_catalog(owner: &Connection, reader: &Connection) -> Result<()> {
    let mut db = Databases::default();
    let schema = owner.text("username");
    let reader_schema = reader.text("username");
    db.query(
        owner,
        r#"CREATE TABLE "items" ("a" NUMBER, "b" NUMBER, CONSTRAINT "items_pk" PRIMARY KEY ("b", "a"));
           INSERT INTO "items" VALUES (1,2);
           CREATE VIEW "items_view" AS SELECT "a", "b" FROM "items";"#,
    )
    .await?;
    db.query(
        owner,
        &format!(r#"GRANT SELECT ON "items" TO "{reader_schema}""#),
    )
    .await?;
    let scoped = tomato_core::database_scope::select(owner, reader_schema);
    let current = db
        .query(
            &scoped,
            "SELECT SYS_CONTEXT('USERENV','CURRENT_SCHEMA') FROM dual",
        )
        .await?;
    ensure!(current[0].rows[0][0] == reader_schema);
    let table = TableId {
        schema: schema.into(),
        name: "items".into(),
    };
    for connection in [&scoped, reader] {
        let read = page(&mut db, connection, &table).await?;
        ensure!(read.total == 1);
        ensure!(read.rows[0][0].display() == "1" && read.rows[0][1].display() == "2");
        ensure!(
            read.definition
                .columns
                .iter()
                .map(|column| (column.name.as_str(), column.primary_key))
                .eq([("a", 2), ("b", 1)])
        );
    }
    let view = page(
        &mut db,
        owner,
        &TableId {
            name: "items_view".into(),
            ..table
        },
    )
    .await?;
    ensure!(view.definition.view && view.total == 1);
    ensure!(
        view.definition
            .columns
            .iter()
            .all(|column| column.primary_key == 0)
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_oracle_transfer_preview_while_server_has_not_replied() -> Result<()> {
    use std::time::Duration;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut connection = Connection::new("ORACLE");
    connection.set("host", "127.0.0.1");
    connection.set("port", listener.local_addr()?.port());
    connection.set("database", "/XEPDB1");
    connection.set("username", "owned_fixture");
    connection.set("password", "owned_fixture");
    let request = table_transfer::Request {
        source: connection.clone(),
        target: connection,
        source_schema: "SRC".into(),
        target_schema: "DST".into(),
        tables: vec!["T".into()],
        structure: true,
        data: true,
        drop_existing: false,
        add: true,
        drop: false,
        modify: true,
    };
    let control = Control::default();
    let cancel = control.clone();
    let preview = tokio::spawn(async move { transfer::preview(&request, &control).await });
    let (peer, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept()).await??;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), preview).await??;
    drop(peer);
    ensure!(result.unwrap_err().to_string().contains("操作已取消"));
    Ok(())
}

async fn run_owned_fixture(catalog_only: bool) -> Result<()> {
    run_owned_fixture_with(|connections, admin| {
        Box::pin(verify(
            &connections[0],
            &connections[1],
            admin,
            catalog_only,
        ))
    })
    .await
}

#[tokio::test]
#[ignore = "Requires an owned disposable loopback Oracle Free/XE container"]
async fn real_oracle_restores_nb3_identity_and_typed_rows() -> Result<()> {
    run_owned_fixture_with(|connections, _| Box::pin(restore_identity_fixture(&connections[0])))
        .await
}

async fn restore_identity_fixture(source: &Connection) -> Result<()> {
    use tomato_services::backup::{Page, Writer};

    let directory = tempfile::tempdir()?;
    let scope = backup::Scope {
        connection: source.name.clone(),
        database: source.text("username").into(),
        path: String::new(),
    };
    let root = directory.path().join(scope.directory()?);
    std::fs::create_dir_all(&root)?;
    let mut writer = Writer::new(&root)?;
    let mut data = Page::new(&root)?;
    let text = "中文 O'Reilly\u{1e}\nC:\\".repeat(1500);
    let binary: Vec<u8> = (0..5001).map(|n| (n % 251) as u8).collect();
    let mut binary_literal = String::new();
    for byte in &binary {
        use std::fmt::Write as _;
        write!(binary_literal, "{byte:02X}")?;
    }
    data.record(|out| {
        write!(
            out,
            "(37,'{}',HEXTORAW('{}'),12345678901234567890.123456789012345678,'2024-02-29 12:34:56.123456789 +08:00',NULL)",
            text.replace('\'', "''"),
            binary_literal
        )?;
        Ok(())
    })?;
    data.record(|out| {
        out.write_all(b"(44,'',HEXTORAW(''),NULL,NULL,'empty LOB')")?;
        Ok(())
    })?;
    let data = writer.page("data.sql.gz", data)?;
    let ddl = format!(
        r#"CREATE TABLE "{}"."items" ("id" NUMBER GENERATED ALWAYS AS IDENTITY (START WITH 37 INCREMENT BY 7) PRIMARY KEY,"label" CLOB,"payload" BLOB,"amount" NUMBER(38,18),"moment" TIMESTAMP(9) WITH TIME ZONE,"missing" VARCHAR2(20))"#,
        scope.database
    );
    let legacy_trigger = format!(
        "CREATE OR REPLACE NONEDITIONABLE TRIGGER \"{owner}\".\"legacy_trigger\" BEFORE INSERT ON \"{owner}\".\"items\" FOR EACH ROW BEGIN NULL; END;\nALTER TRIGGER \"{owner}\".\"legacy_trigger\" DISABLE",
        owner = scope.database
    );
    let definition = backup::Definition {
        meta_version: "30100".into(),
        name: "items".into(),
        kind: "Table".into(),
        ddl,
        sub_ddl: None,
        auto_increment: None,
        fields: Some(
            ["id", "label", "payload", "amount", "moment", "missing"]
                .map(String::from)
                .into(),
        ),
        trigger_ddl: Some(vec![legacy_trigger]),
        index_ddl: None,
        data: Some(vec![data]),
        postgres: None,
        oracle: Some(backup::OracleDefinition {
            schema: scope.database.clone(),
            standard_strings: true,
        }),
    };
    let entry = writer.json("definition.json.gz", &definition)?;
    writer.finish(
        &root.join("identity.nb3"),
        &backup::Metadata {
            meta_version: "30100".into(),
            database_type: "ORACLE".into(),
            schema: scope.database.clone(),
            encryption: "None".into(),
            catalog: String::new(),
            comment: String::new(),
            start_time: "0".into(),
            end_time: "0".into(),
            objects: vec![backup::Object {
                uuid: "identity-fixture".into(),
                kind: "Table".into(),
                name: "items".into(),
                rows: "2".into(),
                metadata: entry,
            }],
        },
    )?;
    let mut db = Databases::default();
    let outcome = restore_archive(
        &mut db,
        source,
        directory.path().into(),
        scope.clone(),
        "identity",
    )
    .await?;
    ensure!(outcome.records == 2);
    let table = TableId {
        schema: scope.database,
        name: "items".into(),
    };
    verify_rows(
        page(&mut db, source, &table).await?.rows,
        vec![
            vec![
                Cell::Text("37".into()),
                Cell::Text(text),
                Cell::Binary(binary),
                Cell::Text("12345678901234567890.123456789012345678".into()),
                Cell::Text("2024-02-29 12:34:56.123456789 +08:00".into()),
                Cell::Null,
            ],
            vec![
                Cell::Text("44".into()),
                Cell::Text(String::new()),
                Cell::Binary(vec![]),
                Cell::Null,
                Cell::Null,
                Cell::Text("empty LOB".into()),
            ],
        ],
    )?;
    let modes = db
        .query(
            source,
            "SELECT generation_type FROM user_tab_identity_cols WHERE table_name='items'",
        )
        .await?;
    ensure!(modes[0].rows[0][0] == "ALWAYS");
    let trigger=db.query(source,"SELECT t.status,o.status FROM user_triggers t JOIN user_objects o ON o.object_name=t.trigger_name AND o.object_type='TRIGGER' WHERE t.trigger_name='legacy_trigger'").await?;
    ensure!(
        trigger[0].rows == [["DISABLED", "VALID"]],
        "legacy trigger was not restored as valid and disabled"
    );
    db.query(source, r#"INSERT INTO "items"("missing") VALUES('next')"#)
        .await?;
    let next = db
        .query(source, r#"SELECT "id" FROM "items" WHERE "missing"='next'"#)
        .await?;
    ensure!(next[0].rows[0][0].parse::<i64>()? > 44);
    db.disconnect(&source.id).await?;
    Ok(())
}

fn verify_rows(mut actual: Vec<Vec<Cell>>, mut expected: Vec<Vec<Cell>>) -> Result<()> {
    fn summary(cell: &Cell) -> String {
        match cell {
            Cell::Text(text) if text.len() > 100 => format!("Text({} bytes)", text.len()),
            Cell::Binary(bytes) => format!("Binary({} bytes)", bytes.len()),
            cell => format!("{cell:?}"),
        }
    }
    ensure!(actual.len() == expected.len(), "restore row count differs");
    actual.sort_by_cached_key(|row| row.first().map(Cell::display));
    expected.sort_by_cached_key(|row| row.first().map(Cell::display));
    for (i, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
        ensure!(
            actual.len() == expected.len(),
            "restore field count differs"
        );
        for (j, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
            ensure!(
                actual == expected,
                "restore row {}, column {}: got {}, expected {}",
                i + 1,
                j + 1,
                summary(actual),
                summary(expected)
            );
        }
    }
    Ok(())
}

async fn restore_archive(
    db: &mut Databases,
    source: &Connection,
    root: std::path::PathBuf,
    scope: backup::Scope,
    name: &str,
) -> Result<backup::Report> {
    let control = Control::default();
    let outcome = db
        .backup(
            source,
            root,
            backup::Operation::Restore(backup::Restore {
                scope,
                name: name.into(),
                namespace: None,
            }),
            Some(control.clone()),
        )
        .await;
    if outcome.is_err()
        && let Some(logs) = control.read(|state| state.logs.clone())
    {
        for line in logs {
            eprintln!("{line}");
        }
    }
    outcome
}

async fn run_owned_fixture_with<F>(verify: F) -> Result<()>
where
    F: for<'a> FnOnce(
        &'a [Connection],
        &'a Connection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>>,
{
    let service = match std::env::var("TOMATO_TEST_ORACLE_ISOLATED").as_deref() {
        Ok("owned-container-freepdb1") => "/FREEPDB1",
        Ok("owned-container-xepdb1") => "/XEPDB1",
        _ => anyhow::bail!("isolated Oracle fixture authorization missing"),
    };
    let port = std::env::var("TOMATO_TEST_ORACLE_PORT")?.parse::<u16>()?;
    ensure!(port > 0, "fixture port missing");
    let mut admin = Connection::new("ORACLE");
    admin.set("host", "127.0.0.1");
    admin.set("port", port);
    admin.set("database", service);
    admin.set("username", "system");
    admin.set("password", std::env::var("TOMATO_TEST_ORACLE_PASSWORD")?);
    if std::env::var("TOMATO_TEST_ORACLE_SSH_ISOLATED").as_deref() == Ok("owned-loopback-linux") {
        let profile = std::path::PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR").context("Owned route profile required")?,
        );
        ensure!(
            profile.is_absolute()
                && profile
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("oracle-1690-ssh-")),
            "Owned SSH fixture profile required"
        );
        let configuration = tomato_core::Store::default_path();
        ensure!(
            !configuration.exists(),
            "SSH fixture configuration must be new"
        );
        let key = std::path::PathBuf::from(
            std::env::var_os("TOMATO_TEST_LINUX_KEY").context("Owned fixture key missing")?,
        );
        ensure!(
            key.is_file() && key.file_name().is_some_and(|n| n == "fixture-ed25519"),
            "Only the owned SSH key may be used"
        );
        let mut jump = Connection::new("SSH");
        jump.set("host", "127.0.0.1");
        jump.set(
            "port",
            std::env::var("TOMATO_TEST_LINUX_SSH_PORT")?.parse::<u16>()?,
        );
        jump.set("username", "tomato");
        jump.set("usePassword", false);
        jump.set("useKey", true);
        jump.set("privateKeyPaths", serde_json::json!([key]));
        admin.set("port", 1521);
        admin.set("useSshTunnel", true);
        admin.set("sshTunnelHostId", jump.id.clone());
        tomato_core::Store {
            connections: vec![jump],
            ..Default::default()
        }
        .save(&configuration)?;
    }
    let label = uuid::Uuid::new_v4().simple().to_string()[..16].to_ascii_uppercase();
    let owners = [format!("TMT_S_{label}"), format!("TMT_D_{label}")];
    let mut connections = Vec::new();
    let mut created = Vec::new();
    let mut db = Databases::default();
    let outcome:Result<()>=async {
        for owner in &owners {
            eprintln!("[oracle fixture] create isolated owner and grant privileges");
            db.query(&admin,&format!("CREATE USER \"{owner}\" IDENTIFIED BY \"OwnedFixture42\" DEFAULT TABLESPACE USERS QUOTA UNLIMITED ON USERS")).await?;
            created.push(owner.clone());
            db.query(&admin,&format!("GRANT CREATE SESSION,CREATE TABLE,CREATE VIEW,CREATE PROCEDURE,CREATE TRIGGER,CREATE SEQUENCE TO \"{owner}\"")).await?;
            let mut connection=admin.clone();connection.id=uuid::Uuid::new_v4().to_string();connection.name=owner.clone();connection.set("username",owner.clone());connection.set("password","OwnedFixture42");connection.set("__tomatoOracleOwner",owner.clone());connection.set("fixtureUnknown",serde_json::json!({"preserve":true}));connections.push(connection);
        }
        verify(&connections,&admin).await
    }.await;
    for connection in &connections {
        let _ = db.disconnect(&connection.id).await;
    }
    let mut cleanup = None;
    for owner in created.into_iter().rev() {
        if let Err(error) = db
            .query(&admin, &format!("DROP USER \"{owner}\" CASCADE"))
            .await
        {
            cleanup = Some(error);
        }
    }
    db.disconnect(&admin.id).await?;
    outcome.context("real Oracle native workflow")?;
    if let Some(error) = cleanup {
        return Err(error).context("owned Oracle fixture cleanup");
    }
    Ok(())
}
