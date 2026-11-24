//! Real designer controls → controller → actor → owned MySQL → sibling data tab.
use super::*;
use crate::tabs::table_design::{Draft, Page};
use native_bridge::Bridge;
use std::time::{Duration, Instant};
mod auxiliary;
mod foreign_names;
mod indexes;
mod object_preload;
mod objects;
mod oracle;
mod oracle_schema;
mod sql_definition;
mod types;
fn draft<'a>(app: &'a TomatoApp, id: &str) -> &'a Draft {
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|tab| tab.id == id).unwrap().panel
    else {
        panic!("SQL tab")
    };
    workbench.designer.as_ref().unwrap()
}
fn draw(app: &mut TomatoApp, ctx: &egui::Context, id: &str) {
    let index = app.tabs.iter().position(|tab| tab.id == id).unwrap();
    let mut ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("owned-designer-root"),
        egui::UiBuilder::new().max_rect(ctx.content_rect()),
    );
    let intent = crate::panels::render(&mut ui, &mut app.tabs[index], &mut app.icons);
    if let Some(intent) = intent {
        app.handle_intent(id, intent);
    }
}
fn frame(app: &mut TomatoApp, id: &str, events: Vec<egui::Event>) {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            events,
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    output.textures_delta.clear();
}
fn click(app: &mut TomatoApp, id: &str, x: f32, y: f32) {
    // Native close requests a parent repaint before the next physical click.
    frame(app, id, vec![]);
    let point = egui::pos2(x, y);
    for pressed in [true, false] {
        frame(
            app,
            id,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(app, id, vec![]);
}
fn sidebar_frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            events,
            ..Default::default()
        },
        |ui| app.connection_list(ui),
    );
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some((text.galley.job.text.clone(), text.pos + egui::vec2(3., 6.)))
            } else {
                None
            }
        })
        .collect();
    output.textures_delta.clear();
    labels
}
fn sidebar_menu(app: &mut TomatoApp, name: &str, action: &str) -> anyhow::Result<()> {
    let rows = sidebar_frame(app, vec![]);
    let point = rows
        .iter()
        .find(|(text, _)| text == name)
        .map(|(_, point)| *point)
        .ok_or_else(|| anyhow::anyhow!("tree row not rendered"))?;
    for pressed in [true, false] {
        sidebar_frame(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let menu = sidebar_frame(app, vec![]);
    let point = menu
        .iter()
        .find(|(text, _)| text == action)
        .map(|(_, point)| *point)
        .ok_or_else(|| anyhow::anyhow!("tree menu action {action} not rendered"))?;
    for pressed in [true, false] {
        sidebar_frame(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    sidebar_frame(app, vec![]);
    Ok(())
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn edit(app: &mut TomatoApp, id: &str, x: f32, y: f32, value: &str) {
    click(app, id, x, y);
    frame(app, id, vec![key(egui::Key::F2)]);
    assert!(
        draft(app, id).edit.is_some(),
        "F2 failed at {x},{y}: page={:?},focus={:?}",
        draft(app, id).page,
        draft(app, id).focus
    );
    frame(
        app,
        id,
        vec![egui::Event::Text(value.into()), key(egui::Key::Enter)],
    );
    frame(app, id, vec![]);
    assert!(
        draft(app, id).edit.is_none(),
        "Enter must finish the editor, not reopen it"
    );
}
#[track_caller]
fn wait(app: &mut TomatoApp) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let oracle = app.store.connections.iter().any(|c| c.kind() == "ORACLE");
    // The isolated Oracle VM uses software CPU emulation. Cold dictionary
    // queries can take over eight minutes; this is not an application timeout.
    let timeout = if oracle { 1200 } else { 30 };
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let caller = std::panic::Location::caller();
    let started = Instant::now();
    if oracle {
        eprintln!("[Oracle designer UI] wait at {caller}");
    }
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(
            Instant::now() < deadline,
            "designer actor timeout at {caller}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    if oracle {
        eprintln!(
            "[Oracle designer UI] complete at {caller} after {:?}",
            started.elapsed()
        );
    }
    Ok(())
}
#[test]
fn preview_focus_cannot_block_field_f2_or_keyboard_column_navigation() -> anyhow::Result<()> {
    use tomato_core::table_design::{Column, Dialect, Metadata, alter::Snapshot};
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut tab = tabs::create_tab(Connection::new("MYSQL"), None)?;
    let mut state = Draft::new("MYSQL", "owned".into())?;
    state.request.table.name = "items".into();
    state.request.columns = vec![
        Column {
            name: "id".into(),
            data_type: "INTEGER".into(),
            ..Default::default()
        },
        Column {
            name: "label".into(),
            data_type: "VARCHAR".into(),
            length: "255".into(),
            ..Default::default()
        },
    ];
    state.load(Box::new(Snapshot {
        definition: state.request.clone(),
        extra_options: Default::default(),
        backslash_escapes: true,
        raw_columns: vec![
            "`id` int DEFAULT NULL".into(),
            "`label` varchar(255) DEFAULT NULL".into(),
        ],
        indexes: Default::default(),
        foreign_keys: Default::default(),
        triggers: Default::default(),
        sql: "CREATE TABLE items(id INT,label VARCHAR(255))".into(),
        view: false,
        primary_name: None,
    }));
    state.receive_metadata(std::sync::Arc::new(Metadata::empty(Dialect::Mysql)));
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    workbench.designer = Some(Box::new(state));
    let id = tab.id.clone();
    app.active = Some(id.clone());
    app.tabs.push(tab);
    frame(&mut app, &id, vec![]);
    click(&mut app, &id, 342., 45.);
    click(&mut app, &id, 120., 70.);
    click(&mut app, &id, 24., 45.);
    edit(&mut app, &id, 750., 133., "真实控件备注");
    assert_eq!(draft(&app, &id).request.columns[1].comment, "真实控件备注");
    click(&mut app, &id, 342., 45.);
    assert!(
        draft(&app, &id).preview.contains("ALTER TABLE"),
        "{}",
        draft(&app, &id).preview
    );
    click(&mut app, &id, 24., 45.);
    click(&mut app, &id, 80., 133.);
    frame(&mut app, &id, vec![key(egui::Key::F2)]);
    frame(
        &mut app,
        &id,
        vec![
            egui::Event::Text("renamed".into()),
            key(egui::Key::ArrowRight),
        ],
    );
    assert_eq!(draft(&app, &id).request.columns[1].name, "renamed");
    assert_eq!(draft(&app, &id).edit.as_ref().unwrap().cell.column, 1);
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    let start = egui::pos2(80., 103.);
    let end = egui::pos2(330., 133.);
    frame(
        &mut app,
        &id,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(&mut app, &id, vec![egui::Event::PointerMoved(end)]);
    frame(
        &mut app,
        &id,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    for row in 0..2 {
        for column in 0..3 {
            assert!(
                draft(&app, &id).selected(row, column),
                "missing ({row},{column}), focus={:?}, rows={:?}",
                draft(&app, &id).focus,
                draft(&app, &id).selected_rows()
            );
        }
    }
    click(&mut app, &id, 880., 200.);
    assert!(draft(&app, &id).selected_rows().is_empty());
    click(&mut app, &id, 80., 133.);
    let Panel::Sql { workbench, .. } =
        &mut app.tabs.iter_mut().find(|tab| tab.id == id).unwrap().panel
    else {
        unreachable!()
    };
    workbench.designer.as_mut().unwrap().copied_fields = Some("stale field cache".into());
    click(&mut app, &id, 150., 422.);
    frame(
        &mut app,
        &id,
        vec![egui::Event::Text("selected text".into())],
    );
    frame(
        &mut app,
        &id,
        vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        }],
    );
    frame(&mut app, &id, vec![egui::Event::Copy]);
    assert!(
        draft(&app, &id).copied_fields.is_none(),
        "copying selected property text must clear the field cache"
    );
    let focused = ctx.memory(|memory| memory.focused()).unwrap();
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focused).is_some());
    app.close(&id);
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focused).is_none());
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
#[test]
#[ignore = "requires owned loopback MySQL fixture and isolated native UI harness"]
fn mysql_designer_controls_create_edit_preview_save_refresh_and_preserve_failed_draft()
-> anyhow::Result<()> {
    designer_roundtrip("MYSQL")
}
#[test]
#[ignore = "requires owned loopback PostgreSQL fixture and isolated native UI harness"]
fn postgres_designer_controls_create_edit_preview_save_refresh_and_preserve_failed_draft()
-> anyhow::Result<()> {
    designer_roundtrip("POSTGRESQL")
}
fn designer_roundtrip(kind: &str) -> anyhow::Result<()> {
    let (variable, prefix, username) = if kind == "MYSQL" {
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
        .ok_or_else(|| anyhow::anyhow!("owned loopback fixture required"))?;
    let (port, database) = endpoint
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("fixture database required"))?;
    let port = port.parse::<u16>()?;
    anyhow::ensure!(
        port > 0
            && database.starts_with("tomato_fixture")
            && database
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "unsafe fixture endpoint"
    );
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(config.clone());
    repository.save(&repository.load()?)?;
    let original = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let bridge = Bridge::new(&ctx);
    let mut base = Connection::new(kind);
    base.name = "owned designer fixture".into();
    base.set("host", "127.0.0.1");
    base.set("port", port);
    base.set("username", username);
    base.set("database", database);
    let owned = format!(
        "tomato_designer_ui_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let mut connection = base.clone();
    if kind == "MYSQL" {
        connection.set("database", owned.clone());
    }
    let qualified = if kind == "MYSQL" {
        format!("`{owned}`.`items`")
    } else {
        format!("\"{owned}\".\"items\"")
    };
    let create_scope = if kind == "MYSQL" {
        format!("CREATE DATABASE `{owned}` CHARACTER SET utf8mb4")
    } else {
        format!("CREATE SCHEMA \"{owned}\"")
    };
    let drop_scope = if kind == "MYSQL" {
        format!("DROP DATABASE IF EXISTS `{owned}`")
    } else {
        format!("DROP SCHEMA IF EXISTS \"{owned}\" CASCADE")
    };
    app.store.connections.push(connection.clone());
    let mut admin = tomato_services::Databases::default();
    let outcome = (|| -> anyhow::Result<()> {
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(&base, &create_scope))?;
        let mut host = crate::app::sql_tree::Host::new(connection.clone());
        host.schemas = std::sync::Arc::new(vec![owned.clone()]);
        host.selected = owned.clone();
        host.tables
            .insert(owned.clone(), std::sync::Arc::new(Vec::new()));
        app.sql_hosts.insert(connection.id.clone(), host);
        sidebar_menu(&mut app, "表", "新建表")?;
        let mut id = app
            .active
            .clone()
            .ok_or_else(|| anyhow::anyhow!("tree New Table did not open a designer"))?;
        wait(&mut app)?;
        assert!(!draft(&app, &id).busy);
        frame(&mut app, &id, vec![]);
        edit(&mut app, &id, 80., 103., "id");
        edit(&mut app, &id, 220., 103., "INTEGER");
        click(&mut app, &id, 334., 16.);
        click(&mut app, &id, 112., 16.);
        frame(
            &mut app,
            &id,
            vec![egui::Event::Text("label".into()), key(egui::Key::Enter)],
        );
        frame(&mut app, &id, vec![]);
        assert_eq!(draft(&app, &id).request.columns[1].name, "label");
        click(&mut app, &id, 340., 45.);
        assert_eq!(draft(&app, &id).page, Page::Sql);
        assert!(draft(&app, &id).preview.contains("CREATE TABLE"));
        click(&mut app, &id, 8., 70.);
        assert!(draft(&app, &id).preview_view.collapsed.contains(&0));
        click(&mut app, &id, 8., 70.);
        assert!(draft(&app, &id).preview_view.collapsed.is_empty());
        click(&mut app, &id, 38., 16.);
        assert!(
            draft(&app, &id).name.is_some(),
            "Save must open the native name dialog"
        );
        bridge.frame(&ctx, vec![], |ctx| draw(&mut app, ctx, &id));
        bridge.frame(
            &ctx,
            vec![egui::Event::Text("items".into()), key(egui::Key::Enter)],
            |ctx| draw(&mut app, ctx, &id),
        );
        wait(&mut app)?;
        assert!(
            draft(&app, &id).saved.is_some(),
            "{:?}",
            app.tabs.iter().find(|t| t.id == id).unwrap().error
        );
        assert!(!draft(&app, &id).dirty);
        bridge.frame(&ctx, vec![], |ctx| draw(&mut app, ctx, &id));
        bridge.frame(&ctx, vec![key(egui::Key::Enter)], |ctx| {
            draw(&mut app, ctx, &id)
        });
        assert!(draft(&app, &id).notice.is_none());
        app.runtime.as_ref().unwrap().block_on(admin.query(
            &connection,
            &format!("INSERT INTO {qualified}(id,label) VALUES(1,'hello')"),
        ))?;
        let table = tomato_core::workbench::TableInfo {
            table: tomato_core::workbench::TableId {
                schema: owned.clone(),
                name: "items".into(),
            },
            view: false,
        };
        sidebar_menu(&mut app, &table.table.name, "打开表")?;
        let sibling = app.active.clone().unwrap();
        wait(&mut app)?;
        sidebar_menu(&mut app, &table.table.name, "设计表")?;
        assert_eq!(
            app.active.as_deref(),
            Some(id.as_str()),
            "created designer must reuse its structure identity"
        );
        indexes::roundtrip(&mut app, &id, kind)?;
        click(&mut app, &id, 24., 45.);
        edit(&mut app, &id, 750., 133., "通过控件保存的备注");
        assert_eq!(
            draft(&app, &id).request.columns[1].comment,
            "通过控件保存的备注",
            "actual comment editor"
        );
        click(&mut app, &id, 342., 45.);
        assert!(
            draft(&app, &id).preview.contains(if kind == "MYSQL" {
                "ALTER TABLE"
            } else {
                "COMMENT ON COLUMN"
            }),
            "preview={}, revision={}, page={:?}",
            draft(&app, &id).preview,
            draft(&app, &id).revision,
            draft(&app, &id).page
        );
        assert!(draft(&app, &id).preview.contains("通过控件保存的备注"));
        click(&mut app, &id, 38., 16.);
        wait(&mut app)?;
        assert!(!draft(&app, &id).dirty);
        assert_eq!(
            draft(&app, &id).saved.as_ref().unwrap().definition.columns[1].comment,
            "通过控件保存的备注"
        );
        let Panel::Sql { workbench, .. } =
            &app.tabs.iter().find(|t| t.id == sibling).unwrap().panel
        else {
            panic!()
        };
        assert!(workbench.page.is_some());
        let mut query = tabs::create_tab(
            connection.clone(),
            app.runtime.as_ref().map(|runtime| runtime.handle()),
        )?;
        let query_id = query.id.clone();
        query.source_id = format!("owned-query:{query_id}");
        let Panel::Sql { workbench, sql, .. } = &mut query.panel else {
            unreachable!()
        };
        workbench.schema = owned.clone();
        *sql = "SELECT id,label FROM items".into();
        app.tabs.push(query);
        app.submit(
            &query_id,
            Command::Database {
                connection: connection.clone(),
                action: tomato_core::workbench::DatabaseAction::Completion {
                    schema: owned.clone(),
                },
            },
            Purpose::Normal,
        );
        wait(&mut app)?;
        click(&mut app, &id, 24., 45.);
        click(&mut app, &id, 112., 16.);
        frame(
            &mut app,
            &id,
            vec![egui::Event::Text("note".into()), key(egui::Key::Enter)],
        );
        click(&mut app, &id, 38., 16.);
        wait(&mut app)?;
        let Panel::Sql { workbench, .. } = &mut app
            .tabs
            .iter_mut()
            .find(|tab| tab.id == sibling)
            .unwrap()
            .panel
        else {
            unreachable!()
        };
        let page = workbench.page.as_ref().unwrap();
        assert_eq!(
            page.definition.columns.len(),
            3,
            "clean data tab must receive the actual new column"
        );
        assert_eq!(page.definition.columns[2].name, "note");
        let mut pending = page.rows[0].clone();
        pending[1] = tomato_core::workbench::Cell::Text("uncommitted data draft".into());
        workbench.edits.insert(0, pending);
        let Panel::Sql { workbench, sql, .. } = &app
            .tabs
            .iter()
            .find(|tab| tab.id == query_id)
            .unwrap()
            .panel
        else {
            unreachable!()
        };
        assert_eq!(sql, "SELECT id,label FROM items");
        assert!(
            workbench
                .completion
                .catalog()
                .tables
                .iter()
                .any(|table| table.name == "items"
                    && table.columns.iter().any(|name| name == "note")),
            "query completion must refresh from the real catalog"
        );
        edit(&mut app, &id, 80., 163., "note2");
        click(&mut app, &id, 38., 16.);
        wait(&mut app)?;
        let Panel::Sql { workbench, .. } =
            &app.tabs.iter().find(|tab| tab.id == sibling).unwrap().panel
        else {
            unreachable!()
        };
        assert!(workbench.needs_refresh && workbench.dirty());
        assert_eq!(
            workbench.edits[&0][1],
            tomato_core::workbench::Cell::Text("uncommitted data draft".into())
        );
        let Panel::Sql { workbench, .. } = &app
            .tabs
            .iter()
            .find(|tab| tab.id == query_id)
            .unwrap()
            .panel
        else {
            unreachable!()
        };
        assert!(
            workbench
                .completion
                .catalog()
                .tables
                .iter()
                .any(|table| table.name == "items"
                    && table.columns.iter().any(|name| name == "note2"))
        );
        let previous = id.clone();
        app.close(&id);
        sidebar_menu(&mut app, "items", "设计表")?;
        id = app.active.clone().unwrap();
        assert_ne!(
            id, previous,
            "closing and opening structure must create a fresh designer"
        );
        wait(&mut app)?;
        assert_eq!(
            draft(&app, &id).saved.as_ref().unwrap().definition.columns[2].name,
            "note2"
        );
        click(&mut app, &id, 24., 45.);
        edit(&mut app, &id, 310., 133., "1");
        // Page-level refresh must retain unrelated unsaved fields.
        app.handle_intent(
            &id,
            Intent::RefreshDesignInfo(tomato_core::table_design::alter::InfoPage::Indexes),
        );
        wait(&mut app)?;
        assert_eq!(draft(&app, &id).request.columns[1].length, "1");
        assert!(draft(&app, &id).dirty);
        click(&mut app, &id, 38., 16.);
        wait(&mut app)?;
        assert!(draft(&app, &id).dirty);
        assert!(!draft(&app, &id).busy);
        assert!(
            draft(&app, &id).notice.as_ref().is_some_and(|n| n.error),
            "failed ALTER must report failure and retain draft"
        );
        assert_eq!(
            draft(&app, &id).saved.as_ref().unwrap().definition.columns[1].length,
            "255"
        );
        let rows = app
            .runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(&connection, &format!("SELECT id,label FROM {qualified}")))?;
        assert_eq!(rows[0].rows[0], ["1", "hello"]);
        app.close(&id);
        auxiliary::exercise(&mut app, &connection, &owned, &mut admin)?;
        foreign_names::exercise(&mut app, &connection, &owned, &mut admin)?;
        sql_definition::exercise(&mut app, &connection, &owned, &mut admin)?;
        objects::exercise(&mut app, &connection, &owned, &bridge, &mut admin)?;
        assert_eq!(std::fs::read(&config)?, original);
        Ok(())
    })();
    // Only this fixture's sessions and database are released.
    let disconnect = app.queue.submit(Command::Disconnect {
        id: connection.id.clone(),
    })?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut disconnected = false;
    while Instant::now() < deadline && !disconnected {
        for result in app.queue.poll() {
            if result.id == disconnect {
                result.result?;
                disconnected = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        let result = admin.query(&base, &drop_scope).await;
        let closed = admin.disconnect(&base.id).await;
        result.map(|_| ()).and(closed)
    });
    outcome.and(cleanup)?;
    anyhow::ensure!(disconnected, "owned designer session did not close");
    Ok(())
}
