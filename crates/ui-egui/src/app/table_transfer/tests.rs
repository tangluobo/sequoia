use super::*;
use crate::app::tests::native_bridge::Bridge;
use std::time::{Duration, Instant};
fn wait(app: &mut TomatoApp) -> anyhow::Result<()> {
    wait_for_tasks(app, Duration::from_secs(45))
}
fn wait_for_tasks(app: &mut TomatoApp, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let ctx = app.context.clone();
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(Instant::now() < deadline, "transfer timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
fn fixture(kind: &str) -> anyhow::Result<()> {
    let (variable, prefix, user) = if kind == "MYSQL" {
        ("TOMATO_TEST_MYSQL_URL", "mysql://root@127.0.0.1:", "root")
    } else {
        (
            "TOMATO_TEST_POSTGRES_URL",
            "postgres://tomato_fixture@127.0.0.1:",
            "tomato_fixture",
        )
    };
    let url = std::env::var(variable)?;
    let endpoint = url
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("owned loopback required"))?;
    let (port, database) = endpoint
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("fixture db"))?;
    anyhow::ensure!(database == "tomato_fixture");
    let port = port.parse::<u16>()?;
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(config.clone());
    repository.save(&repository.load()?)?;
    let before = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut admin = Connection::new(kind);
    admin.name = "owned-transfer".into();
    admin.set("host", "127.0.0.1");
    admin.set("password", "");
    admin.set("port", u64::from(port));
    admin.set("username", user);
    admin.set("database", database);
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.store.connections.push(admin.clone());
    let token = Connection::new("TOOL").id.replace('-', "");
    let scopes = [
        format!("tomato_transfer_ui_a_{token}"),
        format!("tomato_transfer_ui_b_{token}"),
    ];
    let dialect = tomato_services::dialect::resolve(kind)?;
    let sk = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    let mut db = tomato_services::Databases::default();
    app.runtime.as_ref().unwrap().block_on(async{for schema in &scopes{db.query(&admin,&format!("CREATE {sk} {}",dialect.quote(schema)?)).await?;}db.query(&admin,&format!("CREATE TABLE {}.items(id INTEGER PRIMARY KEY,label VARCHAR(100)); INSERT INTO {}.items VALUES(1,'中文传输')",dialect.quote(&scopes[0])?,dialect.quote(&scopes[0])?)).await?;Ok::<_,anyhow::Error>(())})?;
    let outcome = (|| -> anyhow::Result<()> {
        let mut source = admin.clone();
        let mut target = admin.clone();
        if kind == "MYSQL" {
            source.set("database", scopes[0].clone());
            target.set("database", scopes[1].clone());
        }
        // The original immediately copies within the same connection.
        app.copy_table_selection(&source, &scopes[0], vec!["items".into()]);
        app.paste_tables(target.clone(), scopes[1].clone());
        anyhow::ensure!(app.table_transfer.is_none() && app.table_paste.is_some());
        wait(&mut app)?;
        anyhow::ensure!(app.table_paste_notice.as_ref().is_some_and(|n| !n.failed));
        let qualified = format!("{}.items", dialect.quote(&scopes[1])?);
        app.runtime.as_ref().unwrap().block_on(db.query(
            &admin,
            &format!("UPDATE {qualified} SET label='keep destination'"),
        ))?;
        let source_schema = dialect.quote(&scopes[0])?;
        app.runtime.as_ref().unwrap().block_on(db.query(
            &admin,
            &format!("CREATE TABLE {source_schema}.other_items(id INTEGER PRIMARY KEY); INSERT INTO {source_schema}.other_items VALUES(12)"),
        ))?;
        app.runtime.as_ref().unwrap().block_on(db.query(
            &admin,
            &format!("CREATE TABLE {source_schema}.removed(id INTEGER)"),
        ))?;
        app.copy_table_selection(
            &source,
            &scopes[0],
            vec!["removed".into(), "items".into(), "other_items".into()],
        );
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(&admin, &format!("DROP TABLE {source_schema}.removed")))?;
        app.paste_tables(target.clone(), scopes[1].clone());
        wait(&mut app)?;
        anyhow::ensure!(app.table_paste_notice.as_ref().is_some_and(|n| {
            n.failed && n.text.contains("成功 1 张，失败 2 张") && n.text.contains("removed")
        }));
        let rows = app
            .runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(&admin, &format!("SELECT label FROM {qualified}")))?;
        anyhow::ensure!(rows[0].rows == [["keep destination"]]);
        let copied = app.runtime.as_ref().unwrap().block_on(db.query(
            &admin,
            &format!("SELECT id FROM {}.other_items", dialect.quote(&scopes[1])?),
        ))?;
        anyhow::ensure!(copied[0].rows == [["12"]]);
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(&admin, &format!("DROP TABLE {qualified}")))?;

        // Distinct saved connections continue to use the preview and execution wizard.
        target.id = Connection::new(kind).id;
        app.store.connections.push(target.clone());
        app.copy_table_selection(&source, &scopes[0], vec!["items".into()]);
        app.paste_tables(target.clone(), scopes[1].clone());
        wait(&mut app)?;
        let bridge = Bridge::new(&ctx);
        bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
        bridge.click(&ctx, bridge.text("下一步")?, |ctx| {
            app.table_transfer_dialog(ctx)
        });
        wait(&mut app)?;
        let draft = app.table_transfer.as_ref().unwrap();
        anyhow::ensure!(draft.step == 1 && draft.plan.is_some(), "{:?}", draft.error);
        anyhow::ensure!(draft.preview.contains("CREATE TABLE"));
        bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
        bridge.click(&ctx, bridge.text("执行")?, |ctx| {
            app.table_transfer_dialog(ctx)
        });
        wait(&mut app)?;
        let draft = app.table_transfer.as_ref().unwrap();
        anyhow::ensure!(
            draft.completed && !draft.running && draft.error.is_none(),
            "{:?}",
            draft.error
        );
        let rows = app.runtime.as_ref().unwrap().block_on(db.query(
            &admin,
            &format!("SELECT label FROM {}.items", dialect.quote(&scopes[1])?),
        ))?;
        anyhow::ensure!(rows[0].rows[0][0] == "中文传输");
        bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
        bridge.click(&ctx, bridge.text("取消")?, |ctx| {
            app.table_transfer_dialog(ctx)
        });
        anyhow::ensure!(app.table_transfer.is_none());
        anyhow::ensure!(
            std::fs::read(&config)? == before,
            "encrypted configuration changed"
        );
        Ok(())
    })();
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        db.disconnect(&admin.id).await?;
        for scope in scopes.iter().rev() {
            db.query(
                &admin,
                &format!(
                    "DROP {sk} {}{}",
                    dialect.quote(scope)?,
                    if kind == "MYSQL" { "" } else { " CASCADE" }
                ),
            )
            .await?;
        }
        db.disconnect(&admin.id).await?;
        Ok::<_, anyhow::Error>(())
    });
    cleanup?;
    outcome
}
#[test]
#[ignore = "owned loopback database required"]
fn mysql_native_transfer_next_preview_execute_close_and_config_unchanged() -> anyhow::Result<()> {
    fixture("MYSQL")
}
#[test]
#[ignore = "owned loopback database required"]
fn postgres_native_transfer_next_preview_execute_close_and_config_unchanged() -> anyhow::Result<()>
{
    fixture("POSTGRESQL")
}

#[test]
#[ignore = "owned loopback Oracle fixture required"]
fn oracle_native_owner_tree_paste_continues_after_missing_source_and_refreshes_target()
-> anyhow::Result<()> {
    use crate::app::sql_tree::postgres::tests::menu;
    use anyhow::Context;

    let service = match std::env::var("TOMATO_TEST_ORACLE_ISOLATED").as_deref() {
        Ok("owned-container-freepdb1") => "/FREEPDB1",
        Ok("owned-container-xepdb1") => "/XEPDB1",
        _ => anyhow::bail!("owned loopback Oracle fixture required"),
    };
    let port = std::env::var("TOMATO_TEST_ORACLE_PORT")?.parse::<u16>()?;
    anyhow::ensure!(port != 0);
    let mut connection = Connection::new("ORACLE");
    connection.name = "Owned Oracle paste fixture".into();
    connection.set("host", "127.0.0.1");
    connection.set("port", port);
    connection.set("database", service);
    connection.set("username", "system");
    connection.set("password", std::env::var("TOMATO_TEST_ORACLE_PASSWORD")?);

    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(config.clone());
    repository.save(&repository.load()?)?;
    let before = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.store.connections.push(connection.clone());
    let token = Connection::new("TOOL").id.replace('-', "")[..16].to_ascii_uppercase();
    let source = format!("TMT_PASTE_S_{token}");
    let target = format!("TMT_PASTE_D_{token}");
    let mut created = Vec::new();
    let mut db = tomato_services::Databases::default();
    let outcome = (|| -> anyhow::Result<()> {
        app.runtime.as_ref().unwrap().block_on(async {
            for owner in [&source, &target] {
                db.query(&connection, &format!(
                    r#"CREATE USER "{owner}" IDENTIFIED BY "OwnedFixture42" DEFAULT TABLESPACE USERS QUOTA UNLIMITED ON USERS"#
                )).await?;
                created.push(owner);
            }
            for (owner, table, label) in [
                (&source, "gone", "removed after copy"),
                (&source, "exists", "source data"),
                (&source, "copied", "中文内容"),
                (&target, "exists", "keep destination"),
            ] {
                db.query(&connection, &format!(
                    r#"CREATE TABLE "{owner}"."{table}" (id NUMBER PRIMARY KEY, label VARCHAR2(100))"#
                )).await?;
                db.query(&connection, &format!(
                    r#"INSERT INTO "{owner}"."{table}" VALUES(1,'{label}')"#
                )).await?;
            }
            Ok::<_, anyhow::Error>(())
        })?;

        let oracle_timeout = Duration::from_secs(480);
        eprintln!("[Oracle paste] load owner tree");
        app.open_direct_host(connection.clone(), false);
        wait_for_tasks(&mut app, oracle_timeout).context("load Oracle owner tree")?;
        app.filter = target.clone();
        menu(&mut app, &target, 0, "打开").context("open Oracle owner in tree")?;
        eprintln!("[Oracle paste] load target objects");
        wait_for_tasks(&mut app, oracle_timeout).context("load Oracle target objects")?;
        anyhow::ensure!(
            app.sql_hosts[&connection.id].tables[&target]
                .iter()
                .map(|item| item.table.name.as_str())
                .eq(["exists"])
        );
        app.copy_table_selection(
            &connection,
            &source,
            vec!["gone".into(), "exists".into(), "copied".into()],
        );
        app.runtime.as_ref().unwrap().block_on(db.query(
            &connection,
            &format!(r#"DROP TABLE "{source}"."gone" PURGE"#),
        ))?;
        menu(&mut app, &target, 0, "粘贴表").context("paste into Oracle owner through tree")?;
        anyhow::ensure!(
            app.table_paste.is_some() && app.table_transfer.is_none(),
            "the tree menu must submit the immediate paste"
        );
        eprintln!("[Oracle paste] wait for copy and target refresh");
        wait_for_tasks(&mut app, oracle_timeout).context("execute and refresh Oracle paste")?;
        let notice = app
            .table_paste_notice
            .as_ref()
            .context("native paste result")?;
        anyhow::ensure!(
            notice.failed
                && notice.text.contains("gone")
                && notice.text.contains("成功 1 张，失败 2 张"),
            "{}",
            notice.text
        );
        let tables = &app.sql_hosts[&connection.id].tables[&target];
        anyhow::ensure!(tables.iter().any(|item| item.table.name == "copied"));
        anyhow::ensure!(!tables.iter().any(|item| item.table.name == "gone"));
        for (table, value) in [("copied", "中文内容"), ("exists", "keep destination")] {
            let rows = app.runtime.as_ref().unwrap().block_on(db.query(
                &connection,
                &format!(r#"SELECT id, label FROM "{target}"."{table}""#),
            ))?;
            anyhow::ensure!(rows[0].rows == [["1", value]]);
        }
        let bridge = Bridge::new(&ctx);
        bridge.frame(&ctx, vec![], |ctx| app.table_paste_dialog(ctx));
        bridge.click(&ctx, bridge.text("确定")?, |ctx| {
            app.table_paste_dialog(ctx)
        });
        anyhow::ensure!(app.table_paste_notice.is_none());
        anyhow::ensure!(std::fs::read(&config)? == before);
        Ok(())
    })();
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        let mut result = Ok(());
        for owner in created.into_iter().rev() {
            if let Err(error) = db
                .query(&connection, &format!(r#"DROP USER "{owner}" CASCADE"#))
                .await
            {
                result = Err(error);
            }
        }
        db.disconnect(&connection.id).await?;
        result
    });
    cleanup.context("owned Oracle fixture cleanup")?;
    outcome
}
