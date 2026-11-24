//! Exercise the production native wizard layout, scrolling and lifetime without sockets.
use super::*;
use crate::app::tests::native_bridge::Bridge;
#[test]
fn native_wizard_dividers_scroll_preview_and_release_state_without_database_io()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    let source = Connection::new("MYSQL");
    let mut target = source.clone();
    target.id = Connection::new("MYSQL").id;
    let preview = (0..180)
        .map(|i| format!("SELECT {i} AS original_preview_row;\n"))
        .collect::<String>();
    app.table_transfer = Some(Draft {
        id: Connection::new("TOOL").id,
        generation: 0,
        request: Request {
            source,
            target,
            source_schema: "owned".into(),
            target_schema: "target".into(),
            tables: vec!["items".into()],
            structure: true,
            data: true,
            drop_existing: false,
            add: true,
            drop: true,
            modify: true,
        },
        databases: vec![],
        tables: vec![("items".into(), true)],
        step: 1,
        loading: false,
        running: false,
        completed: false,
        plan: None,
        preview: preview.clone(),
        error: None,
        log: String::new(),
        control: Default::default(),
        options: false,
        option_values: [true, true, false],
        editors: Default::default(),
        layout: Default::default(),
        scroll_areas: [None; 2],
    });
    let bridge = Bridge::new(&ctx);
    {
        let draft = app.table_transfer.as_mut().unwrap();
        draft.step = 0;
        draft.databases = vec!["alpha".into(), "beta".into()];
    }
    bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    bridge.click(&ctx, egui::pos2(600., 230.), |ctx| {
        app.table_transfer_dialog(ctx)
    });
    bridge.click(&ctx, bridge.text("beta")?, |ctx| {
        app.table_transfer_dialog(ctx)
    });
    anyhow::ensure!(app.table_transfer.as_ref().unwrap().request.target_schema == "beta");
    anyhow::ensure!(
        app.pending.is_empty(),
        "choosing a destination database must not execute a transfer"
    );
    {
        let draft = app.table_transfer.as_mut().unwrap();
        draft.step = 1;
        draft.preview = preview.clone();
        let table = tomato_core::workbench::TableId {
            schema: "beta".into(),
            name: "items".into(),
        };
        draft.plan = Some(Plan {
            tables: vec![tomato_core::table_transfer::TablePlan {
                source: table.clone(),
                target: table,
                sql: vec!["CREATE TABLE items (id NUMBER)".into()],
                data: true,
                data_sql: Some("INSERT INTO items SELECT id FROM original".into()),
                stamp: String::new(),
            }],
        });
    }
    bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    bridge.text("2 条 SQL，1 张表")?;
    bridge.click(&ctx, bridge.text("复制 SQL")?, |ctx| {
        app.table_transfer_dialog(ctx)
    });
    anyhow::ensure!(bridge.take_copied_text().as_deref() == Some(preview.as_str()));
    app.table_transfer.as_mut().unwrap().preview.clear();
    bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    bridge.click(&ctx, bridge.text("复制 SQL")?, |ctx| {
        app.table_transfer_dialog(ctx)
    });
    anyhow::ensure!(
        bridge.take_copied_text().is_none(),
        "empty SQL must leave the real clipboard unchanged, as in Java"
    );
    app.table_transfer.as_mut().unwrap().preview = preview.clone();
    bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    let drag = |app: &mut TomatoApp, from: egui::Pos2, to: egui::Pos2| {
        for (point, pressed) in [(from, true), (to, true), (to, false)] {
            let mut events = vec![egui::Event::PointerMoved(point)];
            if point == from || !pressed {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            bridge.frame(&ctx, events, |ctx| app.table_transfer_dialog(ctx));
        }
        bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    };
    drag(&mut app, egui::pos2(280., 300.), egui::pos2(305., 300.));
    anyhow::ensure!(
        (app.table_transfer.as_ref().unwrap().layout.left - 305.).abs() < 2.,
        "left divider did not move"
    );
    drag(&mut app, egui::pos2(550., 332.), egui::pos2(550., 370.));
    anyhow::ensure!(
        app.table_transfer.as_ref().unwrap().layout.preview > 0.55,
        "log divider did not move"
    );
    bridge.frame(
        &ctx,
        vec![
            egui::Event::PointerMoved(egui::pos2(600., 200.)),
            egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., -240.),
                modifiers: Default::default(),
            },
        ],
        |ctx| app.table_transfer_dialog(ctx),
    );
    bridge.frame(&ctx, vec![], |ctx| app.table_transfer_dialog(ctx));
    let draft = app.table_transfer.as_ref().unwrap();
    let ids = draft.scroll_areas;
    let scroll = egui::scroll_area::State::load(&ctx, ids[0].unwrap()).unwrap();
    anyhow::ensure!(
        scroll.offset.y > 0.,
        "preview wheel did not scroll actual text"
    );
    anyhow::ensure!(
        draft.preview == preview && app.pending.is_empty(),
        "layout controls must not alter SQL or submit I/O"
    );
    bridge.click(&ctx, bridge.text("取消")?, |ctx| {
        app.table_transfer_dialog(ctx)
    });
    anyhow::ensure!(app.table_transfer.is_none());
    for id in ids.into_iter().flatten() {
        anyhow::ensure!(egui::scroll_area::State::load(&ctx, id).is_none());
    }
    Ok(())
}
