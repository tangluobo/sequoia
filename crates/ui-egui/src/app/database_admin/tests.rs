use super::*;
use crate::app::tests::native_bridge::Bridge;
use anyhow::Context;
use std::time::{Duration, Instant};
fn wait(app: &mut TomatoApp) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let end = Instant::now() + Duration::from_secs(45);
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(Instant::now() < end, "database admin timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
fn workflow(kind: &str) -> anyhow::Result<()> {
    let (var, prefix, user) = if kind == "MYSQL" {
        ("TOMATO_TEST_MYSQL_URL", "mysql://root@127.0.0.1:", "root")
    } else {
        (
            "TOMATO_TEST_POSTGRES_URL",
            "postgres://tomato_fixture@127.0.0.1:",
            "tomato_fixture",
        )
    };
    let url = std::env::var(var)?;
    let (port, database) = url
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("own loopback required"))?
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("db"))?;
    anyhow::ensure!(database == "tomato_fixture");
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
    let mut c = Connection::new(kind);
    c.set("password", "");
    c.name = "owned-admin".into();
    c.set("host", "127.0.0.1");
    c.set("port", u64::from(port.parse::<u16>()?));
    c.set("username", user);
    c.set("database", database);
    app.store.connections.push(c.clone());
    let name = format!(
        "tomato_fixture_admin_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let bridge = Bridge::new(&ctx);
    app.open_database_admin(c.clone(), String::new(), Mode::Create);
    wait(&mut app)?;
    bridge.frame(&ctx, vec![], |ctx| app.database_admin_dialog(ctx));
    bridge.frame(&ctx, vec![egui::Event::Text(name.clone())], |ctx| {
        app.database_admin_dialog(ctx)
    });
    anyhow::ensure!(app.database_admin.as_ref().unwrap().database == name);
    bridge.click(&ctx, bridge.text("SQL预览")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    anyhow::ensure!(
        app.database_admin
            .as_ref()
            .unwrap()
            .request()
            .sql(kind)?
            .unwrap()
            .contains(&name)
    );
    bridge.click(&ctx, bridge.text("确定")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    wait(&mut app)?;
    anyhow::ensure!(
        app.database_admin.is_none(),
        "{:?}",
        app.database_admin.as_ref().and_then(|d| d.error.as_ref())
    );
    let mut backend = tomato_services::Databases::default();
    let exists = app
        .runtime
        .as_ref()
        .unwrap()
        .block_on(backend.workbench(&c, tomato_core::workbench::DatabaseAction::Databases))?;
    let tomato_core::workbench::DatabaseOutput::Databases(names) = exists else {
        unreachable!()
    };
    anyhow::ensure!(names.contains(&name));
    app.open_database_admin(c.clone(), name.clone(), Mode::Edit);
    wait(&mut app)?;
    anyhow::ensure!(!app.database_admin.as_ref().unwrap().info.charset.is_empty());
    bridge.frame(&ctx, vec![], |ctx| app.database_admin_dialog(ctx));
    anyhow::ensure!(
        bridge.text(&name)?.x < app.database_admin.as_ref().unwrap().size()[0] * 0.4,
        "the original read-only database label must stay left aligned"
    );
    bridge.click(&ctx, bridge.text("SQL预览")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    bridge.click(&ctx, bridge.text("保存")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    wait(&mut app)?;
    anyhow::ensure!(
        app.database_admin.is_none(),
        "{:?}",
        app.database_admin.as_ref().and_then(|d| d.error.as_ref())
    );
    let mut routed = c.clone();
    routed.set("database", name.clone());
    app.new_scoped_query(
        routed,
        if kind == "POSTGRESQL" {
            "public".into()
        } else {
            name.clone()
        },
    );
    wait(&mut app)?;
    let query = app.active.clone().unwrap();
    if let Panel::Sql { sql, .. } = &mut app.tabs.iter_mut().find(|t| t.id == query).unwrap().panel
    {
        *sql = "SELECT 'retain unsaved query'".into();
    }
    // No closes or SQL are issued by cancelling the original warning dialog.
    app.open_database_admin(c.clone(), name.clone(), Mode::Drop);
    bridge.frame(&ctx, vec![], |ctx| app.database_admin_dialog(ctx));
    bridge.click(&ctx, bridge.text("否")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    anyhow::ensure!(app.database_admin.is_none());
    app.open_database_admin(c.clone(), name.clone(), Mode::Drop);
    bridge.frame(&ctx, vec![], |ctx| app.database_admin_dialog(ctx));
    bridge.click(&ctx, bridge.text("是")?, |ctx| {
        app.database_admin_dialog(ctx)
    });
    wait(&mut app)?;
    anyhow::ensure!(
        app.database_admin.is_none(),
        "{:?}",
        app.database_admin.as_ref().and_then(|d| d.error.as_ref())
    );
    let exists = app
        .runtime
        .as_ref()
        .unwrap()
        .block_on(backend.workbench(&c, tomato_core::workbench::DatabaseAction::Databases))?;
    let tomato_core::workbench::DatabaseOutput::Databases(names) = exists else {
        unreachable!()
    };
    anyhow::ensure!(!names.contains(&name));
    anyhow::ensure!(
        matches!(&app.tabs.iter().find(|t|t.id==query).context("independent query lost on database deletion")?.panel,Panel::Sql{sql,workbench,..} if sql=="SELECT 'retain unsaved query'"&&!workbench.transaction&&!workbench.selectors.selected)
    );
    app.disconnect_sql(&c.id);
    wait(&mut app)?;
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(backend.disconnect(&c.id))?;
    anyhow::ensure!(std::fs::read(&config)? == before);
    Ok(())
}
#[test]
#[ignore = "owned loopback database required"]
fn mysql_native_database_create_edit_sql_preview_confirm_drop_and_preserve_config()
-> anyhow::Result<()> {
    workflow("MYSQL")
}
#[test]
#[ignore = "owned loopback database required"]
fn postgres_native_database_create_edit_sql_preview_confirm_drop_and_preserve_config()
-> anyhow::Result<()> {
    workflow("POSTGRESQL")
}
