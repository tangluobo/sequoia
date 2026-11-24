//! Real Oracle widgets, save actor, constraint enforcement, and trigger effects.
use super::*;

#[test]
#[ignore = "requires the owned loopback Oracle fixture"]
fn oracle_designer_controls_add_preview_save_readback_and_delete_foreign_keys_and_triggers()
-> anyhow::Result<()> {
    let database = match std::env::var("TOMATO_TEST_ORACLE_ISOLATED").as_deref() {
        Ok("owned-container-xepdb1") => "/XEPDB1",
        Ok("owned-container-freepdb1") => "/FREEPDB1",
        _ => anyhow::bail!("owned Oracle fixture authorization missing"),
    };
    let port = std::env::var("TOMATO_TEST_ORACLE_PORT")?.parse::<u16>()?;
    anyhow::ensure!(port > 0, "loopback fixture port required");
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(file.clone());
    repository.save(&repository.load()?)?;
    let original = std::fs::read(&file)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let mut base = Connection::new("ORACLE");
    base.set("host", "127.0.0.1");
    base.set("port", port);
    base.set("database", database);
    base.set("username", "system");
    base.set("password", std::env::var("TOMATO_TEST_ORACLE_PASSWORD")?);
    let owned = format!(
        "TMT_UI_{}",
        &Connection::new("TOOL").id.replace('-', "")[..16]
    )
    .to_ascii_uppercase();
    let mut connection = tomato_core::database_scope::select(&base, &owned);
    connection.id = Connection::new("ORACLE").id;
    connection.set("username", owned.as_str());
    connection.set("password", "OwnedFixture42");
    app.store.connections.push(connection.clone());
    let mut admin = tomato_services::Databases::default();
    let mut created = false;
    let outcome = (|| -> anyhow::Result<()> {
        let runtime = app.runtime.as_ref().unwrap();
        runtime.block_on(admin.query(&base, &format!("CREATE USER \"{owned}\" IDENTIFIED BY \"OwnedFixture42\" DEFAULT TABLESPACE USERS QUOTA UNLIMITED ON USERS")))?;
        created = true;
        runtime.block_on(admin.query(
            &base,
            &format!("GRANT CREATE SESSION,CREATE TABLE,CREATE VIEW,CREATE TRIGGER TO \"{owned}\""),
        ))?;
        runtime.block_on(admin.query(
            &connection,
            r#"CREATE TABLE "items" ("id" NUMBER PRIMARY KEY,"label" VARCHAR2(100))"#,
        ))?;
        let info = tomato_core::workbench::TableInfo {
            table: tomato_core::workbench::TableId {
                schema: owned.clone(),
                name: "items".into(),
            },
            view: false,
        };
        let mut host = crate::app::sql_tree::Host::new(connection.clone());
        host.schemas = std::sync::Arc::new(vec![owned.clone()]);
        host.selected = owned.clone();
        host.tables
            .insert(owned.clone(), std::sync::Arc::new(vec![info.clone()]));
        app.sql_hosts.insert(connection.id.clone(), host);
        app.open_table(connection.clone(), info, false);
        wait(&mut app)?;
        eprintln!("[Oracle designer UI] real foreign-key and trigger controls");
        auxiliary::exercise(&mut app, &connection, &owned, &mut admin)?;
        sql_definition::exercise(&mut app, &connection, &owned, &mut admin)?;
        sql_definition::exercise_permissions(&mut app, &connection, &base, &owned, &mut admin)?;
        oracle_schema::exercise(&mut app, &connection, &owned, &mut admin)?;
        for tab in &app.tabs {
            if let Panel::Sql { workbench, .. } = &tab.panel
                && let Some(page) = &workbench.page
            {
                assert_eq!(
                    page.total, 1,
                    "designer saves must refresh the sibling data page before subsequent external inserts"
                );
            }
        }
        assert_eq!(std::fs::read(&file)?, original);
        Ok(())
    })();
    let disconnect = app.queue.submit(Command::Disconnect {
        id: connection.id.clone(),
    })?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut closed = false;
    while Instant::now() < deadline && !closed {
        for result in app.queue.poll() {
            if result.id == disconnect {
                result.result?;
                closed = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        admin.disconnect(&connection.id).await?;
        if created {
            admin
                .query(&base, &format!("DROP USER \"{owned}\" CASCADE"))
                .await?;
        }
        admin.disconnect(&base.id).await
    });
    outcome.and(cleanup)?;
    anyhow::ensure!(closed, "owned designer session did not close");
    Ok(())
}
