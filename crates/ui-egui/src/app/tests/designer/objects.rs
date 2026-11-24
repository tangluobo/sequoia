//! Production object page controls against the same exclusively owned database.
use super::*;
use crate::tabs::database_objects::window::{Draft as Browser, Mode};
fn browser<'a>(app: &'a TomatoApp, id: &str) -> &'a Browser {
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == id).unwrap().panel else {
        unreachable!()
    };
    workbench.browser.as_ref().unwrap()
}
#[test]
fn real_object_controls_marquee_delayed_rename_escape_and_editor_release() -> anyhow::Result<()> {
    use tomato_core::{
        database_objects::{Catalog, Object},
        workbench::TableId,
    };
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
    let id = tab.id.clone();
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    let catalog = std::sync::Arc::new(Catalog {
        objects: ["first", "second"]
            .into_iter()
            .map(|name| Object {
                table: TableId {
                    schema: "owned".into(),
                    name: name.into(),
                },
                view: false,
                comment: String::new(),
                engine: String::new(),
                auto_increment: String::new(),
                update_time: String::new(),
                data_length: String::new(),
                rows: String::new(),
                fields: Vec::new(),
            })
            .collect(),
        foreign_keys: Vec::new(),
    });
    let mut state = Browser::default();
    state.receive_shared(catalog.clone());
    workbench.objects = Some(catalog);
    workbench.browser = Some(Box::new(state));
    app.tabs.push(tab);
    app.active = Some(id.clone());
    frame(&mut app, &id, vec![]);
    let start = egui::pos2(240., 150.);
    let end = egui::pos2(20., 40.);
    frame(
        &mut app,
        &id,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
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
            modifiers: Default::default(),
        }],
    );
    assert_eq!(browser(&app, &id).selected.len(), 2);
    click(&mut app, &id, 400., 180.);
    assert!(browser(&app, &id).selected.is_empty());
    click(&mut app, &id, 50., 62.);
    // Move time beyond the double-click interval, then single-click the selection again.
    let render_at = |app: &mut TomatoApp, time: f64| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(896., 650.),
                )),
                time: Some(time),
                ..Default::default()
            },
            |_| draw(app, &ctx, &id),
        );
        output.textures_delta.clear();
    };
    render_at(&mut app, ctx.input(|i| i.time) + 1.);
    click(&mut app, &id, 50., 62.);
    let due = browser(&app, &id).rename_due.unwrap().1;
    render_at(&mut app, due + 0.01);
    assert!(browser(&app, &id).rename.is_some());
    let editor = ctx.memory(|m| m.focused()).unwrap();
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    assert!(browser(&app, &id).rename.is_none());
    assert!(browser(&app, &id).selected.is_empty());
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, editor).is_none());
    click(&mut app, &id, 50., 62.);
    frame(&mut app, &id, vec![key(egui::Key::F2)]);
    frame(&mut app, &id, vec![]);
    assert!(
        browser(&app, &id).rename.is_some(),
        "F2 must work after the focus survives a full frame"
    );
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    let bridge = Bridge::new(&ctx);
    click(&mut app, &id, 50., 62.);
    menu(&mut app, &id, 50., 62., "清空表")?;
    assert_eq!(
        browser(&app, &id).selected.len(),
        1,
        "a popup click must not start an underlying marquee"
    );
    assert_eq!(
        browser(&app, &id).confirmation.as_ref().unwrap().targets[0]
            .table
            .name,
        "first"
    );
    bridge.frame(&ctx, vec![key(egui::Key::Escape)], |ctx| {
        draw(&mut app, ctx, &id)
    });
    assert!(browser(&app, &id).confirmation.is_none());
    assert!(app.pending.is_empty());
    click(&mut app, &id, 760., 16.);
    frame(&mut app, &id, vec![egui::Event::Text("first".into())]);
    let editor = ctx.memory(|m| m.focused()).unwrap();
    app.close(&id);
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, editor).is_none());
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
fn labels(app: &mut TomatoApp, id: &str, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
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
    let labels = output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some((t.galley.job.text.clone(), t.pos + t.galley.size() / 2.))
            } else {
                None
            }
        })
        .collect();
    output.textures_delta.clear();
    labels
}
fn menu(app: &mut TomatoApp, id: &str, x: f32, y: f32, text: &str) -> anyhow::Result<()> {
    frame(app, id, vec![]);
    let pos = egui::pos2(x, y);
    for pressed in [true, false] {
        labels(
            app,
            id,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let texts = labels(app, id, vec![]);
    let point = texts
        .iter()
        .find(|(label, _)| label == text)
        .map(|(_, point)| *point)
        .ok_or_else(|| anyhow::anyhow!("object menu {text} missing: {texts:?}"))?;
    click(app, id, point.x, point.y);
    Ok(())
}
pub(super) fn exercise(
    app: &mut TomatoApp,
    connection: &Connection,
    schema: &str,
    bridge: &Bridge,
    admin: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let quote = if connection.kind() == "MYSQL" {
        '`'
    } else {
        '"'
    };
    let qualified = |name: &str| format!("{quote}{schema}{quote}.{quote}{name}{quote}");
    for sql in [
        format!(
            "CREATE TABLE {} (id INTEGER PRIMARY KEY,label VARCHAR(255))",
            qualified("zz_browser")
        ),
        format!("INSERT INTO {} VALUES(1,'kept')", qualified("zz_browser")),
    ] {
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(connection, &sql))?;
    }
    sidebar_menu(app, "表", "打开对象")?;
    let id = app.active.clone().unwrap();
    wait(app)?;
    assert_eq!(
        crate::app::shell::tab_icon(app.tabs.iter().find(|t| t.id == id).unwrap()),
        Some("images/connect/folder.png")
    );
    if connection.kind() == "POSTGRESQL" {
        assert!(
            app.tabs
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .title
                .contains(&format!("/{schema}("))
        );
    }
    assert_eq!(browser(app, &id).catalog.objects.len(), 2);
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == id).unwrap().panel else {
        unreachable!()
    };
    assert!(std::sync::Arc::ptr_eq(
        &browser(app, &id).catalog,
        workbench.objects.as_ref().unwrap()
    ));
    // Same menu activates the existing page, not another catalog allocation.
    sidebar_menu(app, "表", "打开对象")?;
    assert_eq!(app.active.as_deref(), Some(id.as_str()));
    click(app, &id, 263., 17.);
    wait(app)?;
    let query = app.active.clone().unwrap();
    assert_ne!(query, id);
    let Panel::Sql {
        connection: query_connection,
        workbench,
        ..
    } = &app.tabs.iter().find(|t| t.id == query).unwrap().panel
    else {
        unreachable!()
    };
    assert!(workbench.document.is_some());
    assert_eq!(workbench.schema, schema);
    assert_eq!(
        query_connection.text("database"),
        connection.text("database")
    );
    app.close(&query);
    app.active = Some(id.clone());
    click(app, &id, 50., 62.);
    click(app, &id, 115., 17.);
    wait(app)?;
    let structure = app.active.clone().unwrap();
    assert!(draft(app, &structure).saved.is_some());
    app.close(&structure);
    app.active = Some(id.clone());
    click(app, &id, 862., 639.);
    assert_eq!(browser(app, &id).mode, Mode::Detail);
    click(app, &id, 80., 96.);
    frame(app, &id, vec![key(egui::Key::F2)]);
    frame(app, &id, vec![]);
    assert!(browser(app, &id).rename.is_some());
    frame(
        app,
        &id,
        vec![
            egui::Event::Text("zz_renamed".into()),
            key(egui::Key::Enter),
        ],
    );
    wait(app)?;
    assert!(
        browser(app, &id)
            .catalog
            .objects
            .iter()
            .any(|o| o.table.name == "zz_renamed")
    );
    assert!(
        app.sql_hosts[&connection.id].tables[schema]
            .iter()
            .any(|o| o.table.name == "zz_renamed")
    );
    let rows = app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("SELECT label FROM {}", qualified("zz_renamed")),
    ))?;
    assert_eq!(rows[0].rows[0][0], "kept");
    click(app, &id, 80., 96.);
    click(app, &id, 38., 17.);
    wait(app)?;
    let data_before_clear = app.active.clone().unwrap();
    let Panel::Sql { workbench, .. } = &app
        .tabs
        .iter()
        .find(|t| t.id == data_before_clear)
        .unwrap()
        .panel
    else {
        unreachable!()
    };
    assert_eq!(workbench.page.as_ref().unwrap().rows.len(), 1);
    click(app, &id, 80., 96.);
    menu(app, &id, 80., 96., "清空表")?;
    assert!(browser(app, &id).confirmation.is_some());
    let ctx = app.context.clone();
    bridge.frame(&ctx, vec![], |ctx| draw(app, ctx, &id));
    let cancel = bridge.text("取消")?;
    bridge.click(&ctx, cancel, |ctx| draw(app, ctx, &id));
    assert!(browser(app, &id).confirmation.is_none());
    let count = app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("SELECT count(*) FROM {}", qualified("zz_renamed")),
    ))?;
    assert_eq!(count[0].rows[0][0], "1");
    menu(app, &id, 80., 96., "清空表")?;
    bridge.frame(&ctx, vec![], |ctx| draw(app, ctx, &id));
    let yes = bridge.text("确认清空")?;
    bridge.click(&ctx, yes, |ctx| draw(app, ctx, &id));
    let request = app.pending.values().find_map(|task| match &task.purpose {
        Purpose::ObjectMutation(job) if task.tab_id == id => Some(&job.request),
        _ => None,
    });
    assert!(
        request.is_some(),
        "confirmation did not submit: confirmation={}, selected={:?}, order={:?}, primary={:?}, notice={:?}",
        browser(app, &id).confirmation.is_some(),
        browser(app, &id).selected,
        browser(app, &id).selection_order,
        browser(app, &id).primary,
        browser(app, &id).notice
    );
    assert_eq!(request.unwrap().targets[0].table.name, "zz_renamed");
    wait(app)?;
    let count = app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("SELECT count(*) FROM {}", qualified("zz_renamed")),
    ))?;
    assert_eq!(count[0].rows[0][0], "0");
    let Panel::Sql { workbench, .. } = &app
        .tabs
        .iter()
        .find(|t| t.id == data_before_clear)
        .unwrap()
        .panel
    else {
        unreachable!()
    };
    assert!(
        workbench.page.as_ref().unwrap().rows.is_empty(),
        "clear must refresh the already open data page"
    );
    // Open data, then delete through the native confirmation. The old page remains visible.
    click(app, &id, 80., 96.);
    click(app, &id, 38., 17.);
    wait(app)?;
    let data = app.active.clone().unwrap();
    menu(app, &id, 80., 96., "删除表")?;
    bridge.frame(&ctx, vec![], |ctx| draw(app, ctx, &id));
    let yes = bridge.text("确认删除")?;
    bridge.click(&ctx, yes, |ctx| draw(app, ctx, &id));
    wait(app)?;
    assert!(
        !browser(app, &id)
            .catalog
            .objects
            .iter()
            .any(|o| o.table.name == "zz_renamed")
    );
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == data).unwrap().panel
    else {
        unreachable!()
    };
    assert!(workbench.needs_refresh);
    assert!(workbench.page.is_some());
    // SQL search owns and releases its undo state with the page.
    click(app, &id, 800., 16.);
    frame(app, &id, vec![egui::Event::Text("label".into())]);
    let focused = ctx.memory(|m| m.focused());
    let tab_count = app.tabs.len();
    sidebar_menu(app, &connection.name, "关闭连接")?;
    assert_eq!(
        app.tabs.len(),
        tab_count,
        "disconnect must retain unsaved data drafts"
    );
    assert!(app.error.as_ref().is_some_and(|e| e.contains("未保存")));
    app.error = None;
    for tab in &mut app.tabs {
        if let Panel::Sql {
            connection: other,
            workbench,
            ..
        } = &mut tab.panel
            && other.id == connection.id
        {
            workbench.clear_changes();
        }
    }
    sidebar_menu(app, &connection.name, "关闭连接")?;
    assert!(
        !app.tabs.iter().any(
            |tab| matches!(&tab.panel,Panel::Sql{connection:other,workbench,..}if other.id==connection.id && (workbench.table.is_some() || workbench.browser.is_some() || workbench.designer.is_some()))
        )
    );
    assert!(!app.sql_hosts.contains_key(&connection.id));
    if let Some(focused) = focused {
        assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focused).is_none());
    }
    Ok(())
}
