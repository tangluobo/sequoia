//! Real tree/menu/search controls while an owned HS2 field response is held.
use super::*;
use crate::tabs::database_objects::window::{Draft as Browser, FieldsLoading};

fn browser<'a>(app: &'a TomatoApp, id: &str) -> &'a Browser {
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == id).unwrap().panel else {
        unreachable!()
    };
    workbench.browser.as_ref().unwrap()
}
fn tick(
    app: &mut TomatoApp,
    id: &str,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.05;
    let ctx = app.context.clone();
    app.logic(&ctx);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    output.textures_delta.clear();
    output
}
fn search(app: &mut TomatoApp, id: &str, time: &mut f64, value: &str) {
    let point = egui::pos2(760., 16.);
    for pressed in [true, false] {
        tick(
            app,
            id,
            time,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    tick(
        app,
        id,
        time,
        vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
    );
    tick(
        app,
        id,
        time,
        vec![
            egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            egui::Event::Text(value.into()),
        ],
    );
    for _ in 0..7 {
        tick(app, id, time, vec![]);
    }
    assert_eq!(browser(app, id).search, value);
}

#[test]
#[ignore = "requires owned HS2 field gate fixture"]
fn actual_hive_headers_and_search_are_usable_before_the_background_fields_reply()
-> anyhow::Result<()> {
    let root = PathBuf::from(std::env::var("TOMATO_HIVE_UI_FIXTURE")?);
    anyhow::ensure!(
        root.is_absolute()
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-hive-ui-fixture-")),
        "owned fixture only"
    );
    anyhow::ensure!(std::env::var("TOMATO_HIVE_FIELD_GATE").as_deref() == Ok("owned-fixture"));
    let config = root.join("connections.json");
    anyhow::ensure!(Store::default_path() == config, "isolated profile required");
    let before = std::fs::read(&config)?;
    let c = Store::load(&config)?.connections[0].clone();
    anyhow::ensure!(
        c.kind() == "HIVE" && c.text("host") == "127.0.0.1",
        "loopback only"
    );
    let context = egui::Context::default();
    crate::theme::install(&context);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &context,
        runtime,
        ConnectionFileRepository::new(config.clone()),
    );
    app.open_connection(c.clone());
    wait(&mut app)?;
    app.open_direct_schema(c.clone(), "default".into(), false);
    wait(&mut app)?;
    assert!(app.tabs.is_empty());
    std::fs::write(root.join("arm-fields"), "armed after initial tree")?;
    sidebar_menu(&mut app, "表", "打开对象")?;
    let id = app
        .active
        .clone()
        .ok_or_else(|| anyhow::anyhow!("tree menu did not open objects"))?;
    let mut time = context.input(|i| i.time) + 1.;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("fields-held").exists() {
        tick(&mut app, &id, &mut time, vec![]);
        anyhow::ensure!(
            Instant::now() < deadline,
            "background response was not requested: {:?}",
            app.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = tick(&mut app, &id, &mut time, vec![]);
    assert!(
        out.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == "items")
        )
    );
    assert_eq!(browser(&app, &id).catalog.objects.len(), 2);
    assert!(
        browser(&app, &id)
            .catalog
            .objects
            .iter()
            .all(|o| o.fields.is_empty())
    );
    assert!(browser(&app, &id).fields_loading == FieldsLoading::Pending);
    assert!(!app.tabs.iter().find(|t| t.id == id).unwrap().busy);
    assert!(
        app.pending
            .values()
            .any(|p| p.tab_id == id && matches!(p.purpose, Purpose::ObjectFields(_)))
    );
    search(&mut app, &id, &mut time, "view_items");
    assert_eq!(browser(&app, &id).filtered.len(), 1);
    assert_eq!(
        browser(&app, &id).catalog.objects[browser(&app, &id).filtered[0]]
            .table
            .name,
        "view_items"
    );
    search(&mut app, &id, &mut time, "后台字段备注");
    assert!(browser(&app, &id).filtered.is_empty());
    assert!(browser(&app, &id).state.fields_pending);
    let out = tick(&mut app, &id, &mut time, vec![]);
    assert!(out.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text.contains("字段加载中"))));
    std::fs::write(root.join("release-fields"), "release after live search")?;
    wait(&mut app)?;
    tick(&mut app, &id, &mut time, vec![]);
    assert_eq!(browser(&app, &id).filtered.len(), 1);
    assert_eq!(
        browser(&app, &id).catalog.objects[browser(&app, &id).filtered[0]]
            .table
            .name,
        "items"
    );
    assert!(browser(&app, &id).fields_loading == FieldsLoading::Ready);
    assert!(!browser(&app, &id).state.fields_pending);
    assert!(browser(&app, &id).notice.is_none());
    assert_eq!(std::fs::read(&config)?, before);
    app.disconnect_sql(&c.id);
    wait(&mut app)?;
    app.shutdown_file_transfers();
    drop(app);
    assert_eq!(std::fs::read(&config)?, before);
    std::fs::write(root.join("stop"), "stop")?;
    Ok(())
}
