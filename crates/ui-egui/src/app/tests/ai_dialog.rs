use super::*;
#[test]
fn actual_ai_settings_controls_cancel_without_writing_and_save_encrypted_legacy_fields()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("global.json");
    let original =
        br#"{"aiModel":"old-model","aiApiKey":"old-fixture-key","future":{"preserve":42}}"#;
    std::fs::write(&config, original)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    #[derive(Default)]
    struct Bridge {
        time: f64,
        events: Vec<egui::Event>,
        text: Vec<String>,
    }
    let bridge = std::rc::Rc::new(std::cell::RefCell::new(Bridge::default()));
    let native = bridge.clone();
    ctx.set_embed_viewports(false);
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let (time, events) = {
            let mut native = native.borrow_mut();
            (native.time, std::mem::take(&mut native.events))
        };
        let rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, viewport.builder.inner_size.unwrap());
        let mut input = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(rect),
            time: Some(time),
            events,
            ..Default::default()
        };
        input.viewports.insert(
            viewport.ids.this,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                inner_rect: Some(rect),
                focused: Some(true),
                ..Default::default()
            },
        );
        let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
        native.borrow_mut().text = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
    });
    let frame = |app: &mut TomatoApp, events| {
        let time = {
            let mut native = bridge.borrow_mut();
            native.time += 0.1;
            native.events = events;
            native.time
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                time: Some(time),
                ..Default::default()
            },
            |ui| app.ai_sql_dialog(ui.ctx()),
        );
        output.textures_delta.clear();
    };
    let pointer = |x, y, pressed| {
        vec![
            egui::Event::PointerMoved(egui::pos2(x, y)),
            egui::Event::PointerButton {
                pos: egui::pos2(x, y),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    };
    let edit = |app: &mut TomatoApp, x, y, value: &str| {
        for pressed in [true, false] {
            frame(app, pointer(x, y, pressed));
        }
        frame(
            app,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text(value.into()),
            ],
        );
    };
    app.open_ai_settings(None);
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    edit(&mut app, 100., 94., "discarded-model");
    for pressed in [true, false] {
        frame(&mut app, pointer(510., 271., pressed));
    }
    assert!(app.ai_dialog.is_none());
    assert_eq!(app.settings.extra["aiModel"], "old-model");
    assert_eq!(std::fs::read(&config)?, original);
    for index in 0..4 {
        assert!(
            ctx.data(
                |data| data.get_temp::<egui::widgets::text_edit::TextEditState>(egui::Id::new((
                    "ai-settings",
                    index
                )))
            )
            .is_none()
        );
    }
    app.open_ai_settings(None);
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    edit(&mut app, 100., 94., "new-model");
    edit(&mut app, 100., 129., "new-fixture-key");
    edit(&mut app, 100., 164., "999");
    assert!(
        !bridge
            .borrow()
            .text
            .iter()
            .any(|text| text.contains("new-fixture-key"))
    );
    for pressed in [true, false] {
        frame(&mut app, pointer(425., 271., pressed));
    }
    assert!(
        app.ai_dialog.is_none(),
        "settings save did not complete: {:?}",
        app.error
    );
    let bytes = std::fs::read(&config)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["aiModel"], "new-model");
    assert_eq!(json["aiRequestTimeoutSeconds"], 300);
    assert_eq!(json["future"]["preserve"], 42);
    assert!(
        json["aiApiKey"]
            .as_str()
            .unwrap()
            .starts_with("TOMATO_ENCRYPTED:")
    );
    assert!(!String::from_utf8(bytes)?.contains("new-fixture-key"));
    let settings = tomato_core::ai_sql::settings::Settings::read(&app.settings);
    assert_eq!(settings.key, "new-fixture-key");
    assert_eq!(settings.timeout, 300);
    assert!(app.pending.is_empty());
    assert!(app.queue.poll().next().is_none());
    Ok(())
}
