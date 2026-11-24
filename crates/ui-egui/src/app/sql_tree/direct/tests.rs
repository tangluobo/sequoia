use super::super::postgres::tests::{fold_host_without_queries, menu, wait};
use super::*;
use anyhow::Context;
#[test]
fn oracle_owner_tree_closing_one_owner_keeps_shared_transaction_and_other_tabs()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    let mut connection = Connection::new("ORACLE");
    connection.name = "Oracle native tree".into();
    connection.set("host", "127.0.0.1");
    connection.set("database", "ORCL");
    connection.set("password", "");
    app.store.connections.push(connection.clone());
    app.open_connection(connection.clone());
    assert!(app.tabs.is_empty());
    let task = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p.purpose, Purpose::DirectTree(_)))
        .unwrap()
        .0;
    app.queue.deliver_for_test(
        task,
        Ok(Output::Database(DatabaseOutput::Schemas(vec![
            "OWNER_A".into(),
            "OWNER_B".into(),
        ]))),
    );
    app.logic(&ctx);
    assert!(app.pending.is_empty());
    fold_host_without_queries(&mut app, &connection, "OWNER_A")?;
    app.open_direct_schema(connection.clone(), "OWNER_A".into(), false);
    let task = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p.purpose, Purpose::DirectTree(_)))
        .unwrap()
        .0;
    let table = tomato_core::workbench::TableInfo {
        table: tomato_core::workbench::TableId {
            schema: "OWNER_A".into(),
            name: "items".into(),
        },
        view: false,
    };
    app.queue.deliver_for_test(
        task,
        Ok(Output::Database(DatabaseOutput::Tables(vec![
            table.clone(),
        ]))),
    );
    app.logic(&ctx);
    assert!(app.tabs.is_empty());
    assert_eq!(
        app.sql_hosts[&connection.id].connection.text("database"),
        "ORCL"
    );
    for owner in ["OWNER_A", "OWNER_B"] {
        let mut tab = tabs::create_tab(connection.clone(), None)?;
        if let Panel::Sql { workbench, .. } = &mut tab.panel {
            workbench.schema = owner.into();
            workbench.table = Some(tomato_core::workbench::TableId {
                schema: owner.into(),
                name: "items".into(),
            });
        }
        app.tabs.push(tab);
    }
    app.sql_hosts
        .get_mut(&connection.id)
        .unwrap()
        .transactions
        .insert("ORCL".into());
    app.close_direct_database(connection.clone(), "OWNER_A".into());
    assert_eq!(app.tabs.len(), 1);
    assert!(matches!(&app.tabs[0].panel,Panel::Sql{workbench,..} if workbench.schema=="OWNER_B"));
    assert!(app.sql_hosts[&connection.id].transactions.contains("ORCL"));
    assert!(
        !app.pending.values().any(
            |p| matches!(&p.purpose,Purpose::DirectTree(job) if matches!(job.kind,Kind::Close))
        )
    );
    Ok(())
}
#[test]
#[ignore = "owned MySQL loopback database required"]
fn mysql_native_tree_opens_without_query_and_keeps_query_transaction_on_database_close()
-> anyhow::Result<()> {
    let url = std::env::var("TOMATO_TEST_MYSQL_URL")?;
    let (auth, endpoint) = url
        .strip_prefix("mysql://")
        .context("fixture scheme")?
        .split_once("@127.0.0.1:")
        .context("owned fixture only")?;
    let (port, database) = endpoint.split_once('/').context("fixture database")?;
    anyhow::ensure!(database == "tomato_fixture");
    let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
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
    let mut admin = Connection::new("MYSQL");
    admin.name = "owned-mysql-tree".into();
    admin.set("host", "127.0.0.1");
    admin.set("port", u64::from(port.parse::<u16>()?));
    admin.set("database", "tomato_fixture");
    admin.set("username", username);
    admin.set(
        "password",
        percent_encoding::percent_decode_str(password)
            .decode_utf8()?
            .into_owned(),
    );
    app.store.connections.push(admin.clone());
    let token = Connection::new("TOOL").id.replace('-', "");
    let name = format!("tomato_fixture_tree_{token}");
    let extra_database = format!("tomato_fixture_tree_more_{token}");
    let mut backend = tomato_services::Databases::default();
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(backend.query(&admin, &format!("CREATE DATABASE {name}")))?;
    let mut routed = admin.clone();
    routed.set("database", name.clone());
    app.runtime.as_ref().unwrap().block_on(backend.query(
        &routed,
        "CREATE TABLE marker(id INTEGER PRIMARY KEY); INSERT INTO marker VALUES(1)",
    ))?;
    let outcome = (|| -> anyhow::Result<()> {
        app.open_connection(admin.clone());
        wait(&mut app)?;
        anyhow::ensure!(
            app.tabs.is_empty(),
            "connecting a host must only open the database tree"
        );
        fold_host_without_queries(&mut app, &admin, &name)?;
        menu(&mut app, &name, 0, "打开")?;
        wait(&mut app)?;
        anyhow::ensure!(app.tabs.is_empty());
        anyhow::ensure!(
            app.sql_hosts[&admin.id].tables[&name]
                .iter()
                .any(|t| t.table.name == "marker")
        );
        app.open_table(
            routed.clone(),
            tomato_core::workbench::TableInfo {
                table: tomato_core::workbench::TableId {
                    schema: name.clone(),
                    name: "marker".into(),
                },
                view: false,
            },
            false,
        );
        wait(&mut app)?;
        let data_tab = app.active.clone().unwrap();
        app.new_scoped_query(routed.clone(), name.clone());
        wait(&mut app)?;
        let query = app.active.clone().context("query")?;
        if let Panel::Sql { sql, .. } = &mut app
            .tabs
            .iter_mut()
            .find(|tab| tab.id == query)
            .unwrap()
            .panel
        {
            *sql = "SELECT 'unsaved'".into();
        }
        app.submit(
            &query,
            Command::Database {
                connection: routed.clone(),
                action: DatabaseAction::Transaction {
                    action: tomato_core::workbench::TransactionAction::Begin,
                },
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        app.submit(
            &query,
            Command::Sql {
                connection: routed.clone(),
                sql: "UPDATE marker SET id=2".into(),
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        menu(&mut app, &name, 0, "关闭")?;
        wait(&mut app)?;
        anyhow::ensure!(app.error.is_none(), "{:?}", app.error);
        anyhow::ensure!(!app.tabs.iter().any(|tab| tab.id == data_tab));
        let kept = app
            .tabs
            .iter()
            .find(|tab| tab.id == query)
            .context("query must survive")?;
        anyhow::ensure!(
            matches!(&kept.panel, Panel::Sql { sql,workbench,.. } if sql=="SELECT 'unsaved'" && workbench.transaction)
        );
        anyhow::ensure!(!app.sql_hosts[&admin.id].tables.contains_key(&name));
        app.submit(
            &query,
            Command::Sql {
                connection: routed.clone(),
                sql: "SELECT id FROM marker".into(),
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        let kept = app.tabs.iter().find(|tab| tab.id == query).unwrap();
        anyhow::ensure!(
            matches!(&kept.panel, Panel::Sql { results,.. } if results.last().is_some_and(|r|r.rows.first().is_some_and(|r|r[0]=="2")))
        );
        // Closing the last query also leaves the explicitly connected tree/session alive.
        app.close_tab(&query, true);
        anyhow::ensure!(app.sql_hosts[&admin.id].transactions.contains(&name));
        menu(&mut app, &name, 0, "打开")?;
        wait(&mut app)?;
        app.new_scoped_query(routed.clone(), name.clone());
        wait(&mut app)?;
        let reopened = app.active.clone().unwrap();
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t|t.id==reopened).unwrap().panel, Panel::Sql{workbench,..} if workbench.transaction)
        );
        app.submit(
            &reopened,
            Command::Database {
                connection: routed.clone(),
                action: DatabaseAction::Transaction {
                    action: tomato_core::workbench::TransactionAction::Rollback,
                },
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        menu(&mut app, &name, 0, "刷新")?;
        wait(&mut app)?;
        menu(&mut app, &admin.name, 0, "关闭连接")?;
        wait(&mut app)?;
        anyhow::ensure!(!app.sql_hosts.contains_key(&admin.id));
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t| t.id == reopened).context("query must survive host close")?.panel, Panel::Sql {workbench,..} if !workbench.transaction)
        );
        let tabs_before = app.tabs.len();
        app.open_connection(admin.clone());
        wait(&mut app)?;
        anyhow::ensure!(
            app.tabs.len() == tabs_before,
            "host reconnect cannot create a query"
        );
        menu(&mut app, &name, 0, "打开")?;
        wait(&mut app)?;
        anyhow::ensure!(!app.sql_hosts[&admin.id].schemas.contains(&extra_database));
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(backend.query(&admin, &format!("CREATE DATABASE {extra_database}")))?;
        menu(&mut app, &admin.name, 0, "刷新")?;
        wait(&mut app)?;
        let host = &app.sql_hosts[&admin.id];
        anyhow::ensure!(
            host.schemas.contains(&extra_database)
                && host.tables.is_empty()
                && host.direct.as_ref().unwrap().scopes.is_empty()
        );
        anyhow::ensure!(
            app.tabs.iter().any(|tab| tab.id == reopened),
            "host refresh cannot close query editors"
        );
        anyhow::ensure!(std::fs::read(&config)? == before);
        Ok(())
    })();
    app.disconnect_sql(&admin.id);
    wait(&mut app)?;
    app.runtime.as_ref().unwrap().block_on(async {
        backend.disconnect(&admin.id).await?;
        backend
            .query(&admin, &format!("DROP DATABASE IF EXISTS {extra_database}"))
            .await?;
        backend
            .query(&admin, &format!("DROP DATABASE {name}"))
            .await?;
        backend.disconnect(&admin.id).await
    })?;
    outcome
}
