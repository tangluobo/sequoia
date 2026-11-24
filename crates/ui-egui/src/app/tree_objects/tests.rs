//! Actual sidebar gestures and native confirmations against owned loopback databases.
use super::*;
use crate::app::{
    sql_tree::postgres::tests::{menu, wait},
    tests::native_bridge::Bridge,
};
use anyhow::Context;
fn frame(
    app: &mut TomatoApp,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> Vec<(String, egui::Pos2)> {
    let ctx = app.context.clone();
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 900.),
            )),
            time: Some(ctx.input(|i| i.time) + 0.3),
            events,
            ..Default::default()
        },
        |ui| app.connection_list(ui),
    );
    let text = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some((t.galley.job.text.clone(), t.pos + egui::vec2(3., 6.)))
            } else {
                None
            }
        })
        .collect();
    out.textures_delta.clear();
    text
}
fn click(app: &mut TomatoApp, point: egui::Pos2, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
            modifiers,
        );
    }
}
fn point(app: &mut TomatoApp, text: &str) -> anyhow::Result<egui::Pos2> {
    Ok(frame(app, vec![], Default::default())
        .into_iter()
        .find(|(label, _)| label == text)
        .with_context(|| format!("missing sidebar row: {text}"))?
        .1)
}
fn expand(app: &mut TomatoApp, text: &str) -> anyhow::Result<()> {
    let point = point(app, text)? - egui::vec2(35., 0.);
    click(app, point, Default::default());
    Ok(())
}
fn answer(app: &mut TomatoApp, bridge: &Bridge, text: &str) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    bridge.frame(&ctx, vec![], |ctx| app.tree_objects_dialog(ctx));
    bridge.click(&ctx, bridge.text(text)?, |ctx| app.tree_objects_dialog(ctx));
    Ok(())
}
fn fixture(kind: &str) -> anyhow::Result<()> {
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
    let endpoint = url.strip_prefix(prefix).context("owned loopback only")?;
    let (port, database) = endpoint.split_once('/').context("database")?;
    anyhow::ensure!(database == "tomato_fixture");
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(path.clone());
    repository.save(&repository.load()?)?;
    let before = std::fs::read(&path)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let mut connection = Connection::new(kind);
    connection.name = "owned-tree-actions".into();
    connection.set("host", "127.0.0.1");
    connection.set("port", u64::from(port.parse::<u16>()?));
    connection.set("username", user);
    connection.set("password", "");
    connection.set("database", database);
    app.store.connections.push(connection.clone());
    let schema = format!(
        "tomato_fixture_actions_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let namespace = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    let dialect = tomato_services::dialect::resolve(kind)?;
    let qschema = dialect.quote(&schema)?;
    let qualified = |name: &str| -> anyhow::Result<String> {
        Ok(format!("{qschema}.{}", dialect.quote(name)?))
    };
    let mut backend = tomato_services::Databases::default();
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(backend.query(&connection, &format!("CREATE {namespace} {qschema}")))?;
    let mut routed = connection.clone();
    if kind == "MYSQL" {
        routed.set("database", schema.clone());
    }
    app.runtime.as_ref().unwrap().block_on(async {
        for name in ["tree_action","tree_batch_a","tree_batch_b"] {
            backend.query(&routed,&format!("CREATE TABLE {}(id INTEGER PRIMARY KEY,label VARCHAR(50));INSERT INTO {} VALUES(1,'kept')",qualified(name)?,qualified(name)?)).await?;
        }
        backend.query(&routed,&format!("CREATE VIEW {} AS SELECT * FROM {}",qualified("tree_view")?,qualified("tree_action")?)).await?;
        Ok::<_,anyhow::Error>(())
    })?;
    let outcome = (|| -> anyhow::Result<()> {
        app.open_connection(connection.clone());
        wait(&mut app)?;
        if kind == "POSTGRESQL" {
            menu(&mut app, database, 0, "打开")?;
            wait(&mut app)?;
        }
        menu(&mut app, &schema, 0, "打开")?;
        wait(&mut app)?;
        expand(&mut app, "表")?;
        wait(&mut app)?;
        menu(&mut app, "tree_action", 0, "打开表")?;
        wait(&mut app)?;
        let data = app.active.clone().context("data tab")?;
        let bridge = Bridge::new(&ctx);
        menu(&mut app, "tree_action", 0, "清空表")?;
        anyhow::ensure!(app.tree_objects.is_some() && app.pending.is_empty());
        answer(&mut app, &bridge, "取消")?;
        anyhow::ensure!(app.tree_objects.is_none() && app.pending.is_empty());
        let count = app.runtime.as_ref().unwrap().block_on(backend.query(
            &routed,
            &format!("SELECT count(*) FROM {}", qualified("tree_action")?),
        ))?;
        anyhow::ensure!(count[0].rows[0][0] == "1");
        menu(&mut app, "tree_action", 0, "清空表")?;
        answer(&mut app, &bridge, "确认清空")?;
        wait(&mut app)?;
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t|t.id==data).unwrap().panel,Panel::Sql{workbench,..} if workbench.page.as_ref().is_some_and(|p|p.rows.is_empty())),
            "sidebar clear must refresh the open data page"
        );
        expand(&mut app, "表")?;
        app.runtime.as_ref().unwrap().block_on(backend.query(
            &routed,
            &format!(
                "INSERT INTO {} VALUES(2,'truncate')",
                qualified("tree_action")?
            ),
        ))?;
        menu(&mut app, "tree_action", 0, "截断表")?;
        answer(&mut app, &bridge, "确认截断")?;
        wait(&mut app)?;
        let count = app.runtime.as_ref().unwrap().block_on(backend.query(
            &routed,
            &format!("SELECT count(*) FROM {}", qualified("tree_action")?),
        ))?;
        anyhow::ensure!(count[0].rows[0][0] == "0");
        expand(&mut app, "表")?;
        menu(&mut app, "tree_action", 0, "新建表")?;
        wait(&mut app)?;
        let new = app.active.clone().context("designer")?;
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t|t.id==new).unwrap().panel,Panel::Sql{workbench,..} if workbench.designer.as_ref().is_some_and(|d|!d.created))
        );
        app.close_tab(&new, true);
        let first = point(&mut app, "tree_batch_a")?;
        click(&mut app, first, Default::default());
        let second = point(&mut app, "tree_batch_b")?;
        click(
            &mut app,
            second,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        );
        anyhow::ensure!(
            app.sql_selection.len() == 2,
            "Ctrl selection must retain both tree tables"
        );
        menu(&mut app, "tree_batch_b", 0, "删除表")?;
        anyhow::ensure!(app.tree_objects.as_ref().unwrap().request.targets.len() == 2);
        // The confirmation owns its original target snapshot.
        app.sql_selection.clear();
        answer(&mut app, &bridge, "取消")?;
        let selected = point(&mut app, "tree_batch_a")?;
        click(&mut app, selected, Default::default());
        click(
            &mut app,
            selected,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        );
        anyhow::ensure!(app.sql_selection.is_empty());
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            Default::default(),
        );
        anyhow::ensure!(
            app.tree_objects.is_none() && app.pending.is_empty(),
            "Delete after deselecting every row must not fall back to an old target"
        );
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            }],
            Default::default(),
        );
        let first = point(&mut app, "tree_batch_a")?;
        click(&mut app, first, Default::default());
        let second = point(&mut app, "tree_batch_b")?;
        click(
            &mut app,
            second,
            egui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        anyhow::ensure!(
            app.sql_selection.len() == 2,
            "Shift must select the visible table range"
        );
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            Default::default(),
        );
        anyhow::ensure!(
            app.tree_objects
                .as_ref()
                .is_some_and(|d| d.request.targets.len() == 2),
            "Delete key must use selected table snapshot"
        );
        answer(&mut app, &bridge, "确认删除")?;
        wait(&mut app)?;
        let tables = app.runtime.as_ref().unwrap().block_on(backend.workbench(
            &routed,
            tomato_core::workbench::DatabaseAction::Tables {
                schema: schema.clone(),
            },
        ))?;
        anyhow::ensure!(
            matches!(tables,DatabaseOutput::Tables(ref t) if !t.iter().any(|i|i.table.name.starts_with("tree_batch")))
        );
        expand(&mut app, "视图")?;
        menu(&mut app, "tree_view", 0, "删除表")?;
        answer(&mut app, &bridge, "确认删除")?;
        wait(&mut app)?;
        let tables = app.runtime.as_ref().unwrap().block_on(backend.workbench(
            &routed,
            tomato_core::workbench::DatabaseAction::Tables {
                schema: schema.clone(),
            },
        ))?;
        anyhow::ensure!(
            matches!(tables,DatabaseOutput::Tables(ref t) if !t.iter().any(|i|i.table.name=="tree_view"))
        );
        anyhow::ensure!(
            app.error.is_none() && app.tree_objects.is_none(),
            "{:?}",
            app.error
        );
        anyhow::ensure!(std::fs::read(&path)? == before);
        Ok(())
    })();
    app.disconnect_sql(&connection.id);
    wait(&mut app)?;
    app.runtime.as_ref().unwrap().block_on(async {
        backend.disconnect(&connection.id).await?;
        backend
            .query(
                &connection,
                &format!(
                    "DROP {namespace} {qschema}{}",
                    if kind == "POSTGRESQL" { " CASCADE" } else { "" }
                ),
            )
            .await?;
        backend.disconnect(&connection.id).await
    })?;
    outcome
}
#[test]
#[ignore = "owned MySQL loopback database required"]
fn mysql_tree_table_menus_confirm_cancel_multiselect_and_refresh_real_data() -> anyhow::Result<()> {
    fixture("MYSQL")
}
#[test]
#[ignore = "owned PostgreSQL loopback database required"]
fn postgres_tree_table_menus_confirm_cancel_multiselect_and_refresh_real_data() -> anyhow::Result<()>
{
    fixture("POSTGRESQL")
}
