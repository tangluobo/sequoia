//! Exercise the real sidebar event path without ever contacting a saved server.
use super::super::*;
use super::postgres::tests::{click, frame};
use tomato_core::workbench::DatabaseOutput;

fn reply(app: &mut TomatoApp, output: DatabaseOutput) {
    assert_eq!(
        app.pending.len(),
        1,
        "only the requested catalog may be queued"
    );
    let id = *app.pending.keys().next().unwrap();
    let pending = app.pending.remove(&id).unwrap();
    match pending.purpose {
        Purpose::DirectTree(job) => app.direct_tree_reply(job, Ok(Output::Database(output))),
        Purpose::PgTree(job) => app.pg_tree_reply(job, Ok(Output::Database(output))),
        _ => panic!("unexpected background catalog request"),
    }
    assert!(app.error.is_none(), "{:?}", app.error);
}

fn double_click(app: &mut TomatoApp, label: &str) {
    // Separate this gesture from earlier single-click assertions. Otherwise
    // egui correctly classifies the synthetic sequence as a triple click.
    for _ in 0..45 {
        frame(app, vec![]);
    }
    let point = frame(app, vec![])
        .into_iter()
        .find(|(text, _)| text == label)
        .unwrap()
        .1;
    for _ in 0..2 {
        click(app, point, egui::PointerButton::Primary);
    }
}

#[test]
fn database_catalog_is_lazy_through_search_single_click_and_double_click() -> anyhow::Result<()> {
    for kind in ["MYSQL", "ORACLE", "POSTGRESQL", "HIVE"] {
        let directory = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        // This runtime is deliberately never polled: requests are captured, not sent.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(directory.path().join("connections.json")),
        );
        let mut connection = Connection::new(kind);
        connection.name = "owned lazy tree".into();
        connection.set("password", "");
        connection.set("database", "unopened_default");
        app.store.connections.push(connection.clone());
        app.open_connection(connection.clone());
        let names = (0..200)
            .map(|index| format!("db_{index:03}"))
            .collect::<Vec<_>>();
        reply(
            &mut app,
            if kind == "POSTGRESQL" {
                DatabaseOutput::Databases(names)
            } else {
                DatabaseOutput::Schemas(names)
            },
        );
        let host = &app.sql_hosts[&connection.id];
        assert!(host.tables.is_empty());
        assert!(
            host.direct
                .as_ref()
                .is_none_or(|tree| tree.scopes.is_empty())
        );
        assert!(
            host.pg
                .as_ref()
                .is_none_or(|tree| tree.databases.is_empty())
        );
        assert!(app.tabs.is_empty());
        app.filter = "db_000".into();
        let labels = frame(&mut app, vec![]);
        assert!(labels.iter().any(|(text, _)| text == "db_000"));
        assert!(
            !labels
                .iter()
                .any(|(text, _)| ["表", "视图", "函数", "查询", "备份"].contains(&text.as_str())),
            "search invented children for {kind}"
        );
        let point = labels.iter().find(|(text, _)| text == "db_000").unwrap().1;
        click(&mut app, point, egui::PointerButton::Primary);
        assert!(app.pending.is_empty(), "single click opened {kind}");
        click(
            &mut app,
            point - egui::vec2(31., 0.),
            egui::PointerButton::Primary,
        );
        assert!(
            app.pending.is_empty(),
            "unopened database has an active disclosure arrow: {kind}"
        );
        app.filter.clear();
        double_click(&mut app, "db_000");
        assert_eq!(
            app.pending.len(),
            1,
            "double click must open exactly one scope: {kind}"
        );
        let mut routed = connection.clone();
        if kind == "POSTGRESQL" {
            reply(
                &mut app,
                DatabaseOutput::Schemas(vec!["public".into(), "untouched_schema".into()]),
            );
            let pg = app.sql_hosts[&connection.id].pg.as_ref().unwrap();
            assert_eq!(pg.databases.len(), 1);
            assert!(pg.databases["db_000"].scopes.is_empty());
            let point = frame(&mut app, vec![])
                .into_iter()
                .find(|(text, _)| text == "public")
                .unwrap()
                .1;
            click(&mut app, point, egui::PointerButton::Primary);
            assert!(app.pending.is_empty());
            click(
                &mut app,
                point - egui::vec2(31., 0.),
                egui::PointerButton::Primary,
            );
            assert!(app.pending.is_empty());
            double_click(&mut app, "public");
            routed.set("database", "db_000");
        }
        reply(&mut app, DatabaseOutput::Tables(vec![]));
        let host = &app.sql_hosts[&connection.id];
        if let Some(pg) = &host.pg {
            assert_eq!(pg.databases.len(), 1);
            assert_eq!(pg.databases["db_000"].scopes.len(), 1);
            app.open_pg_schema(routed.clone(), "public".into(), false);
        } else {
            assert_eq!(host.tables.len(), 1);
            assert!(host.tables.contains_key("db_000"));
            app.open_direct_schema(connection.clone(), "db_000".into(), false);
        }
        assert!(
            app.pending.is_empty(),
            "an empty loaded scope must not reload"
        );
        app.open_connection(connection.clone());
        assert!(
            app.pending.is_empty(),
            "reopening a loaded host must not fetch descendants"
        );
        assert!(app.tabs.is_empty());
        assert_eq!(
            app.store.connections[0].text("database"),
            "unopened_default"
        );
        let labels = frame(&mut app, vec![]);
        assert_eq!(
            labels.iter().any(|(text, _)| text == "备份"),
            kind != "HIVE",
            "backup folder capability for {kind}"
        );
    }
    Ok(())
}

#[test]
fn empty_database_list_is_loaded_once_until_explicit_refresh() -> anyhow::Result<()> {
    for kind in ["MYSQL", "ORACLE", "POSTGRESQL", "HIVE"] {
        let directory = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(directory.path().join("connections.json")),
        );
        let mut connection = Connection::new(kind);
        connection.set("password", "");
        app.open_connection(connection.clone());
        reply(
            &mut app,
            if kind == "POSTGRESQL" {
                DatabaseOutput::Databases(vec![])
            } else {
                DatabaseOutput::Schemas(vec![])
            },
        );
        app.open_connection(connection.clone());
        assert!(app.pending.is_empty());
        app.refresh_sql_host(connection);
        assert_eq!(app.pending.len(), 1);
    }
    Ok(())
}
