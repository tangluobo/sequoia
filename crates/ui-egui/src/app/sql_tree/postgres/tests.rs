use super::*;
use std::time::{Duration, Instant};
pub(in crate::app) fn wait(app: &mut TomatoApp) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(Instant::now() < deadline, "tree timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
pub(in crate::app) fn frame(
    app: &mut TomatoApp,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let ctx = app.context.clone();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 900.),
            )),
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
pub(in crate::app) fn click(app: &mut TomatoApp, point: egui::Pos2, button: egui::PointerButton) {
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
pub(in crate::app) fn fold_host_without_queries(
    app: &mut TomatoApp,
    connection: &Connection,
    child: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(app.tabs.is_empty() && app.pending.is_empty());
    for _ in 0..3 {
        let point = frame(app, vec![])
            .into_iter()
            .find(|(text, _)| text == &connection.name)
            .context("host label")?
            .1
            - egui::vec2(31., 0.);
        click(app, point, egui::PointerButton::Primary);
        anyhow::ensure!(
            app.collapsed.contains(&connection.id),
            "host arrow must fold without opening another session"
        );
        anyhow::ensure!(
            !frame(app, vec![]).iter().any(|(text, _)| text == child),
            "folded children remain visible"
        );
        anyhow::ensure!(app.pending.is_empty() && app.tabs.is_empty());
        click(app, point, egui::PointerButton::Primary);
        anyhow::ensure!(!app.collapsed.contains(&connection.id));
        anyhow::ensure!(frame(app, vec![]).iter().any(|(text, _)| text == child));
        anyhow::ensure!(app.pending.is_empty() && app.tabs.is_empty());
    }
    Ok(())
}
pub(in crate::app) fn menu(
    app: &mut TomatoApp,
    name: &str,
    nth: usize,
    action: &str,
) -> anyhow::Result<()> {
    let text = frame(app, vec![]);
    let point = text
        .iter()
        .filter(|(s, _)| s == name)
        .nth(nth)
        .context("tree row missing")?
        .1;
    click(app, point, egui::PointerButton::Secondary);
    let text = frame(app, vec![]);
    let point = text
        .iter()
        .find(|(s, _)| s == action)
        .context("tree menu missing")?
        .1;
    click(app, point, egui::PointerButton::Primary);
    Ok(())
}
use anyhow::Context;
#[test]
#[ignore = "owned PostgreSQL loopback database required"]
fn postgres_native_tree_database_scope_icons_refresh_close_and_reopen() -> anyhow::Result<()> {
    let url = std::env::var("TOMATO_TEST_POSTGRES_URL")?;
    let endpoint = url
        .strip_prefix("postgres://tomato_fixture@127.0.0.1:")
        .context("own fixture")?;
    let (port, db) = endpoint.split_once('/').context("db")?;
    anyhow::ensure!(db == "tomato_fixture");
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
    let mut admin = Connection::new("POSTGRESQL");
    admin.name = "owned-tree".into();
    admin.set("host", "127.0.0.1");
    admin.set("port", u64::from(port.parse::<u16>()?));
    admin.set("database", db);
    admin.set("username", "tomato_fixture");
    app.store.connections.push(admin.clone());
    let token = Connection::new("TOOL").id.replace('-', "");
    let names = [
        format!("tomato_fixture_tree_a_{token}"),
        format!("tomato_fixture_tree_b_{token}"),
    ];
    let extra_database = format!("tomato_fixture_tree_c_{token}");
    let mut backend = tomato_services::Databases::default();
    let mut routed = Vec::new();
    app.runtime.as_ref().unwrap().block_on(async{for name in &names{backend.query(&admin,&format!("CREATE DATABASE {name}")).await?;let mut c=admin.clone();c.set("database",name.clone());backend.query(&c,"CREATE TABLE public.marker(id INTEGER PRIMARY KEY); INSERT INTO public.marker VALUES(1)").await?;routed.push(c);}Ok::<_,anyhow::Error>(())})?;
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(backend.query(&routed[0], "CREATE SCHEMA fixture_aux"))?;
    let backup_scope = tomato_core::backup::Scope {
        connection: admin.name.clone(),
        database: names[0].clone(),
        path: String::new(),
    };
    let backup_directory = directory.path().join(backup_scope.directory()?);
    std::fs::create_dir_all(&backup_directory)?;
    std::fs::write(
        backup_directory.join("shared_listing.nb3"),
        b"owned directory-list fixture; never restored",
    )?;
    let outcome = (|| -> anyhow::Result<()> {
        app.open_connection(admin.clone());
        anyhow::ensure!(
            app.credential.is_some() && app.pending.is_empty(),
            "missing saved password must prompt before network I/O"
        );
        let password_frame = |app: &mut TomatoApp, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 800.),
                    )),
                    time: Some(ctx.input(|i| i.time) + 0.1),
                    events,
                    ..Default::default()
                },
                |_| app.credential_dialog(&ctx),
            );
            let labels = out
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(t) = &shape.shape {
                        Some((t.galley.job.text.clone(), t.pos + t.galley.size() / 2.))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            out.textures_delta.clear();
            labels
        };
        password_frame(&mut app, vec![]);
        let labels = password_frame(
            &mut app,
            vec![egui::Event::Text("owned-fixture-session-password".into())],
        );
        let accept = labels
            .iter()
            .find(|(text, _)| text == "确定")
            .context("password confirmation missing")?
            .1;
        for pressed in [true, false] {
            password_frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(accept),
                    egui::Event::PointerButton {
                        pos: accept,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        anyhow::ensure!(
            app.credential.is_none(),
            "password prompt did not resume host open"
        );
        wait(&mut app)?;
        anyhow::ensure!(
            app.sql_hosts[&admin.id].connection.text("password")
                == "owned-fixture-session-password"
        );
        anyhow::ensure!(
            app.store
                .connections
                .iter()
                .find(|c| c.id == admin.id)
                .unwrap()
                .options
                .get("password")
                .is_none_or(serde_json::Value::is_null)
        );
        anyhow::ensure!(
            app.tabs.is_empty(),
            "opening PG tree must not create a query editor"
        );
        fold_host_without_queries(&mut app, &admin, &names[0])?;
        for (i, name) in names.iter().enumerate() {
            menu(&mut app, name, 0, "打开")?;
            wait(&mut app)?;
            menu(&mut app, "public", i, "打开")?;
            wait(&mut app)?;
        }
        menu(&mut app, "fixture_aux", 0, "打开")?;
        wait(&mut app)?;
        frame(&mut app, vec![]);
        wait(&mut app)?;
        let backup_key = super::super::super::backup_tree::Key::new(&admin.id, &names[0], "");
        for namespace in ["public", "fixture_aux"] {
            anyhow::ensure!(
                app.collapsed.contains(&backup_key.view_id(Some(namespace))),
                "each namespace must initially fold its shared on-disk backup directory"
            );
        }
        let pg = app.sql_hosts[&admin.id].pg.as_ref().unwrap();
        for (c, name) in routed.iter().zip(&names) {
            let scope = &pg.databases[name].scopes["public"];
            anyhow::ensure!(
                scope
                    .tables
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|t| t.table.name == "marker")
            );
            anyhow::ensure!(!app.collapsed.contains(&schema_key(c, "public")));
        }
        anyhow::ensure!(schema_key(&routed[0], "public") != schema_key(&routed[1], "public"));
        let rows = super::super::super::db_tree::rows(
            &app.store,
            &app.tabs,
            &app.collapsed,
            super::super::super::db_tree::Hosts {
                remote: &app.remote_hosts,
                backups: &app.backups,
                queries: &app.queries,
                sql: &app.sql_hosts,
                ssh: &app.ssh_hosts,
                local: &app.local_trees,
                mongo: &app.mongo_hosts,
                redis: &app.redis_hosts,
            },
            "",
        );
        for name in &names {
            let row=rows.iter().find(|r|matches!(r.node,super::super::super::db_tree::Node::Database(_,db,_)if db==name)).unwrap();
            anyhow::ensure!(row.icon() == "images/connect/database.png");
        }
        anyhow::ensure!(
            rows.iter()
                .filter(|r| matches!(r.node, super::super::super::db_tree::Node::Objects(..)))
                .count()
                == 9
        );
        drop(rows);
        app.open_table(
            routed[1].clone(),
            TableInfo {
                table: tomato_core::workbench::TableId {
                    schema: "public".into(),
                    name: "marker".into(),
                },
                view: false,
            },
            false,
        );
        wait(&mut app)?;
        let tab = app.active.clone().unwrap();
        // A dirty independent query and its transaction survive closing the tree database.
        app.new_scoped_query(routed[0].clone(), "public".into());
        wait(&mut app)?;
        let query = app.active.clone().context("query tab")?;
        app.choose_sql_connection(&query, &admin.name);
        wait(&mut app)?;
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t|t.id==query).unwrap().panel,Panel::Sql{connection:c,..} if c.text("database")==names[0] && c.text("password")=="owned-fixture-session-password"),
            "query toolbar must retain live credentials and the preferred actual database"
        );
        if let Panel::Sql { sql, .. } = &mut app
            .tabs
            .iter_mut()
            .find(|tab| tab.id == query)
            .unwrap()
            .panel
        {
            *sql = "SELECT 'keep this draft'".into();
        }
        app.submit(
            &query,
            Command::Database {
                connection: routed[0].clone(),
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
                connection: routed[0].clone(),
                sql: "UPDATE public.marker SET id=2".into(),
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        menu(&mut app, &names[0], 0, "关闭")?;
        wait(&mut app)?;
        anyhow::ensure!(app.tabs.iter().any(|t| t.id == tab));
        let kept = app
            .tabs
            .iter()
            .find(|t| t.id == query)
            .context("query lost when database closed")?;
        anyhow::ensure!(
            matches!(&kept.panel, Panel::Sql { sql, workbench, .. } if sql == "SELECT 'keep this draft'" && workbench.transaction)
        );
        anyhow::ensure!(app.sql_hosts[&admin.id].transactions.contains(&names[0]));
        app.submit(
            &query,
            Command::Sql {
                connection: routed[0].clone(),
                sql: "SELECT id FROM public.marker".into(),
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        let kept = app.tabs.iter().find(|t| t.id == query).unwrap();
        anyhow::ensure!(
            matches!(&kept.panel, Panel::Sql { results, .. } if results.last().is_some_and(|r|r.rows.first().is_some_and(|row|row[0]=="2")))
        );
        app.submit(
            &query,
            Command::Database {
                connection: routed[0].clone(),
                action: DatabaseAction::Transaction {
                    action: tomato_core::workbench::TransactionAction::Rollback,
                },
            },
            Purpose::Normal,
        );
        wait(&mut app)?;

        anyhow::ensure!(
            !app.sql_hosts[&admin.id]
                .pg
                .as_ref()
                .unwrap()
                .databases
                .contains_key(&names[0])
        );
        menu(&mut app, &names[0], 0, "打开")?;
        wait(&mut app)?;
        menu(&mut app, "public", 0, "打开")?;
        wait(&mut app)?;
        menu(&mut app, &names[0], 0, "刷新")?;
        wait(&mut app)?;
        anyhow::ensure!(
            app.sql_hosts[&admin.id].pg.as_ref().unwrap().databases[&names[0]]
                .scopes
                .is_empty()
        );
        anyhow::ensure!(
            app.sql_hosts[&admin.id].pg.as_ref().unwrap().databases[&names[1]]
                .scopes
                .contains_key("public")
        );
        app.disconnect_sql(&admin.id);
        wait(&mut app)?;
        anyhow::ensure!(!app.sql_hosts.contains_key(&admin.id));
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t| t.id == query).context("query must survive host close")?.panel, Panel::Sql {workbench,..} if !workbench.transaction)
        );
        // Selecting a saved connection with no live host must prompt before any SQL I/O,
        // then continue this same editor instead of creating a root query tab.
        app.choose_sql_connection(&query, &admin.name);
        anyhow::ensure!(app.credential.is_some() && app.pending.is_empty());
        password_frame(&mut app, vec![]);
        let labels = password_frame(
            &mut app,
            vec![egui::Event::Text("owned-toolbar-session-password".into())],
        );
        let field = ctx
            .memory(|memory| memory.focused())
            .context("password field focus")?;
        anyhow::ensure!(egui::widgets::text_edit::TextEditState::load(&ctx, field).is_some());
        let accept = labels
            .iter()
            .find(|(text, _)| text == "确定")
            .context("toolbar credential confirmation")?
            .1;
        for pressed in [true, false] {
            password_frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(accept),
                    egui::Event::PointerButton {
                        pos: accept,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        anyhow::ensure!(app.credential.is_none());
        anyhow::ensure!(
            egui::widgets::text_edit::TextEditState::load(&ctx, field).is_none(),
            "closed password prompt retained its editor state"
        );
        wait(&mut app)?;
        let resumed = match &app.tabs.iter().find(|t| t.id == query).unwrap().panel {
            Panel::Sql {
                connection: c, sql, ..
            } => {
                anyhow::ensure!(
                    c.text("password") == "owned-toolbar-session-password"
                        && c.text("database") == names[0]
                        && sql == "SELECT 'keep this draft'"
                );
                c.clone()
            }
            _ => unreachable!(),
        };
        app.submit(
            &query,
            Command::Sql {
                connection: resumed,
                sql: "SELECT id FROM public.marker".into(),
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        anyhow::ensure!(
            matches!(&app.tabs.iter().find(|t|t.id==query).unwrap().panel,Panel::Sql{results,..} if results.last().is_some_and(|r|r.rows.first().is_some_and(|row|row[0]=="1")))
        );
        menu(&mut app, &names[0], 0, "打开")?;
        wait(&mut app)?;
        menu(&mut app, "public", 0, "打开")?;
        wait(&mut app)?;
        anyhow::ensure!(
            !app.sql_hosts[&admin.id]
                .pg
                .as_ref()
                .unwrap()
                .names
                .contains(&extra_database)
        );
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(backend.query(&admin, &format!("CREATE DATABASE {extra_database}")))?;
        let tabs_before = app
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        menu(&mut app, &admin.name, 0, "刷新")?;
        wait(&mut app)?;
        let pg = app.sql_hosts[&admin.id].pg.as_ref().unwrap();
        anyhow::ensure!(
            pg.names.contains(&extra_database) && pg.databases.is_empty(),
            "host refresh must read new databases and reset opened directory nodes"
        );
        anyhow::ensure!(
            tabs_before
                .iter()
                .all(|id| app.tabs.iter().any(|tab| &tab.id == id)),
            "directory refresh must retain independent editors"
        );
        app.disconnect_sql(&admin.id);
        wait(&mut app)?;
        anyhow::ensure!(std::fs::read(&config)? == before);
        Ok(())
    })();
    app.disconnect_sql(&admin.id);
    wait(&mut app)?;
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        backend.disconnect(&admin.id).await?;
        backend
            .query(&admin, &format!("DROP DATABASE IF EXISTS {extra_database}"))
            .await?;
        for name in names.iter().rev() {
            backend
                .query(&admin, &format!("DROP DATABASE {name}"))
                .await?;
        }
        backend.disconnect(&admin.id).await?;
        Ok::<_, anyhow::Error>(())
    });
    cleanup?;
    outcome
}
