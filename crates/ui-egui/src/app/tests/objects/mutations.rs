use super::*;
use crate::app::{db_tree, sql_tree, tests::native_bridge::Bridge};
use tomato_core::workbench::{ColumnFilter, DatabaseAction, Sort, TableId, TableInfo};

#[test]
#[ignore = "requires owned MySQL fixture"]
fn mysql_native_object_mutation_refresh() -> anyhow::Result<()> {
    fixture("MYSQL", "TOMATO_TEST_MYSQL_URL", "mysql", "root")
}

#[test]
#[ignore = "requires owned PostgreSQL fixture"]
fn pg_native_object_mutation_refresh() -> anyhow::Result<()> {
    fixture(
        "POSTGRESQL",
        "TOMATO_TEST_POSTGRES_URL",
        "postgres",
        "tomato_fixture",
    )
}

fn fixture(kind: &str, variable: &str, scheme: &str, user: &str) -> anyhow::Result<()> {
    let url = std::env::var(variable)?;
    let prefix = format!("{scheme}://{user}@127.0.0.1:");
    let (port, database) = url
        .strip_prefix(&prefix)
        .and_then(|tail| tail.split_once('/'))
        .context("owned loopback fixture required")?;
    anyhow::ensure!(database == "tomato_fixture");
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(&config);
    repository.save(&repository.load()?)?;
    let before = std::fs::read(&config)?;
    let context = egui::Context::default();
    crate::theme::install(&context);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    let mut app = TomatoApp::with_repository(&context, runtime, repository);
    let mut connection = Connection::new(kind);
    connection.set("host", "127.0.0.1");
    connection.set("port", port.parse::<u16>()?);
    connection.set("username", user);
    connection.set("password", "");
    connection.set("database", database);
    app.store.connections.push(connection.clone());
    app.sql_hosts.insert(
        connection.id.clone(),
        sql_tree::Host::new(connection.clone()),
    );
    let schema = format!(
        "tomato_mutations_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let dialect = tomato_services::dialect::resolve(kind)?;
    let quoted = dialect.quote(&schema)?;
    let q = |name| -> anyhow::Result<String> { Ok(format!("{quoted}.{}", dialect.quote(name)?)) };
    let namespace = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    let mut db = tomato_services::Databases::default();
    handle.block_on(db.query(&connection, &format!("CREATE {namespace} {quoted}")))?;
    let result = (|| {
        let table = q("sample_table")?;
        let view = q("sample_view")?;
        handle.block_on(db.query(
            &connection,
            &format!(
                "CREATE TABLE {table}(id INTEGER PRIMARY KEY); \
                 INSERT INTO {table} VALUES(1),(2),(3); \
                 CREATE VIEW {view} AS SELECT * FROM {table}"
            ),
        ))?;
        let mut controls = Controls {
            context: context.clone(),
            time: 10.,
        };
        let other_catalog = if kind == "POSTGRESQL" {
            app.open_objects(connection.clone(), "public".into());
            controls.wait(&mut app)?;
            Some(
                workbench(&app.tabs[0])
                    .objects
                    .clone()
                    .context("public catalog")?,
            )
        } else {
            None
        };
        app.open_objects(connection.clone(), schema.clone());
        controls.wait(&mut app)?;
        let objects = app.active.clone().context("object browser")?;
        let info = |name: &str, view| TableInfo {
            table: TableId {
                schema: schema.clone(),
                name: name.into(),
            },
            view,
        };
        let table_info = info("sample_table", false);
        app.open_table(connection.clone(), table_info, false);
        let data = app.active.clone().context("data tab")?;
        controls.wait(&mut app)?;
        let mut request = workbench(app.tabs.iter().find(|tab| tab.id == data).unwrap())
            .request()
            .context("page request")?;
        request.query.filters.push(ColumnFilter {
            column: "id".into(),
            condition: tomato_core::result_filter::Condition::parse(Some("> 1")),
        });
        request.query.sorts.push(Sort {
            column: "id".into(),
            descending: true,
        });
        app.submit(
            &data,
            Command::Database {
                connection: tabs::workbench::query_connection(&connection, &schema),
                action: DatabaseAction::Page { request },
            },
            Purpose::Normal,
        );
        controls.wait(&mut app)?;
        anyhow::ensure!(data_tab(&app, &data).page.as_ref().unwrap().rows[0][0].display() == "3");

        app.active = Some(objects.clone());
        let view_point = |frame: egui::FullOutput| {
            frame
                .shapes
                .into_iter()
                .find_map(|s| match s.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "sample_view" => {
                        Some(text.pos + text.galley.size() / 2.)
                    }
                    _ => None,
                })
                .context("view tile")
        };
        controls.click(&mut app, "sample_table", false, false)?;
        let point = view_point(controls.frame(&mut app, vec![]))?;
        controls.click_at(&mut app, point, false, egui::Modifiers::CTRL);
        let object_workbench = workbench(app.tabs.iter().find(|tab| tab.id == objects).unwrap());
        anyhow::ensure!(object_workbench.browser.as_ref().unwrap().selected.len() == 2);
        controls.click(&mut app, "sample_view", true, false)?;
        controls.click(&mut app, "清空表", false, true)?;
        let draft = workbench(app.tabs.iter().find(|tab| tab.id == objects).unwrap())
            .browser
            .as_ref()
            .unwrap();
        let targets = &draft
            .confirmation
            .as_ref()
            .context("clear confirmation")?
            .targets;
        anyhow::ensure!(targets.len() == 1 && targets[0].table.name == "sample_table");
        let bridge = Bridge::new(&context);
        confirm(&bridge, &mut app, "取消")?;
        controls.wait(&mut app)?;
        anyhow::ensure!(data_tab(&app, &data).page.as_ref().unwrap().rows.len() == 2);
        controls.click(&mut app, "sample_view", true, false)?;
        controls.click(&mut app, "清空表", false, true)?;
        confirm(&bridge, &mut app, "确认清空")?;
        controls.wait(&mut app)?;
        let cleared = data_tab(&app, &data).page.as_ref().unwrap();
        anyhow::ensure!(cleared.rows.is_empty() && cleared.offset == 0);
        anyhow::ensure!(cleared.query.filters.len() == 1 && cleared.query.sorts.len() == 1);
        anyhow::ensure!(cleared.sql.to_ascii_uppercase().contains("ORDER BY"));
        let count =
            handle.block_on(db.query(&connection, &format!("SELECT COUNT(*) FROM {view}")))?;
        anyhow::ensure!(count[0].value(0, 0) == Some("0"), "view must remain intact");

        handle.block_on(db.query(&connection, &format!("INSERT INTO {table} VALUES(8)")))?;
        let Panel::Sql {
            workbench: draft, ..
        } = &mut app
            .tabs
            .iter_mut()
            .find(|tab| tab.id == data)
            .unwrap()
            .panel
        else {
            unreachable!()
        };
        draft
            .inserted
            .push(vec![Some(tomato_core::workbench::Cell::Text("9".into()))]);
        controls.click(&mut app, "sample_table", true, false)?;
        controls.click(&mut app, "截断表", false, true)?;
        confirm(&bridge, &mut app, "确认截断")?;
        controls.wait(&mut app)?;
        let count =
            handle.block_on(db.query(&connection, &format!("SELECT COUNT(*) FROM {table}")))?;
        anyhow::ensure!(count[0].value(0, 0) == Some("0"));
        let draft = data_tab(&app, &data);
        anyhow::ensure!(draft.inserted.len() == 1 && draft.needs_refresh);

        let key = |info: &TableInfo| {
            db_tree::Row {
                node: db_tree::Node::Table(&connection, info),
                depth: 0,
            }
            .key()
        };
        let old = key(&info("sample_view", true));
        let renamed = "renamed_视图";
        let new = key(&info(renamed, true));
        controls.click(&mut app, "sample_view", false, false)?;
        app.selected_connection = Some(old.clone());
        app.sql_anchor = Some(old.clone());
        app.sql_selection.insert(old.clone());
        controls.frame(&mut app, vec![key_event(egui::Key::F2)]);
        controls.frame(&mut app, vec![]);
        controls.frame(&mut app, vec![egui::Event::Text(renamed.into())]);
        controls.frame(&mut app, vec![key_event(egui::Key::Enter)]);
        // Close before polling the queued SQL result, even if the server replies immediately.
        anyhow::ensure!(app.pending.values().any(|task| {
            task.tab_id == objects && matches!(task.purpose, Purpose::ObjectMutation(_))
        }));
        app.close(&objects);
        anyhow::ensure!(app.tabs.iter().all(|tab| tab.id != objects));
        controls.wait(&mut app)?;
        anyhow::ensure!(app.selected_connection.as_ref() == Some(&new));
        anyhow::ensure!(app.sql_anchor.as_ref() == Some(&new) && app.sql_selection.contains(&new));
        let cached = app.sql_hosts[&connection.id]
            .tables_for(&connection, &schema)
            .context("sidebar catalog without the originating tab")?;
        anyhow::ensure!(
            cached
                .iter()
                .any(|item| item.view && item.table.name == renamed)
        );
        anyhow::ensure!(!cached.iter().any(|item| item.table.name == "sample_view"));
        let renamed_view = q(renamed)?;
        let rows = handle
            .block_on(db.query(&connection, &format!("SELECT COUNT(*) FROM {renamed_view}")))?;
        anyhow::ensure!(rows[0].value(0, 0) == Some("0"));
        app.open_objects(connection.clone(), schema.clone());
        controls.wait(&mut app)?;
        controls.click(&mut app, renamed, true, false)?;
        controls.click(&mut app, "删除表", false, true)?;
        confirm(&bridge, &mut app, "确认删除")?;
        controls.wait(&mut app)?;
        anyhow::ensure!(app.sql_anchor.is_none() && !app.sql_selection.contains(&new));
        anyhow::ensure!(
            handle
                .block_on(db.query(&connection, &format!("SELECT COUNT(*) FROM {renamed_view}")))
                .is_err()
        );
        if let Some(other) = other_catalog {
            anyhow::ensure!(
                std::sync::Arc::ptr_eq(&other, workbench(&app.tabs[0]).objects.as_ref().unwrap(),),
                "other PostgreSQL schema must keep its catalog"
            );
        }
        anyhow::ensure!(std::fs::read(&config)? == before);
        Ok(())
    })();
    let cleanup = handle.block_on(db.query(
        &connection,
        &format!(
            "DROP {namespace} {quoted}{}",
            if kind == "POSTGRESQL" { " CASCADE" } else { "" }
        ),
    ));
    result?;
    cleanup?;
    Ok(())
}

fn data_tab<'a>(app: &'a TomatoApp, id: &str) -> &'a crate::tabs::workbench::Workbench {
    workbench(app.tabs.iter().find(|tab| tab.id == id).unwrap())
}

fn key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

fn confirm(bridge: &Bridge, app: &mut TomatoApp, button: &str) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
    let pos = bridge.text(button)?;
    for pressed in [true, false] {
        bridge.root_frame(
            &ctx,
            vec![],
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            |ui| app.ui(ui),
        );
    }
    Ok(())
}
