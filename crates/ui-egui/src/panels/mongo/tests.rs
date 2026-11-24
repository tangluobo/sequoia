use super::*;
#[test]
fn new_document_receives_keyboard_input_without_an_extra_editor_click() {
    let ctx = egui::Context::default();
    let connection = tomato_core::Connection::new("MONGODB");
    let mut tab = Tab {
        id: connection.id.clone(),
        source_id: "mongo-focus-fixture".into(),
        title: "items@fixture".into(),
        panel: Panel::Mongo {
            connection: std::sync::Arc::new(connection),
            view: Box::new(State::new(tomato_services::mongo::Target {
                database: "fixture".into(),
                collection: "items".into(),
            })),
        },
        busy: false,
        error: None,
    };
    if let Panel::Mongo { view, .. } = &mut tab.panel {
        view.pending.clear();
    } else {
        panic!("Expected Mongo workspace");
    }
    let frame = |tab: &mut Tab, mut events: Vec<egui::Event>| {
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                egui::Event::Key { modifiers, .. }
                | egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 900.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                render(ui, tab);
            },
        );
        output.textures_delta.clear();
        output
    };
    let output = frame(&mut tab, vec![]);
    let p = output
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "新建文档").then_some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        })
        .unwrap();
    for pressed in [true, false] {
        frame(
            &mut tab,
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
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
    frame(
        &mut tab,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            egui::Event::Text("{\"name\":\"中文\"}".into()),
        ],
    );
    let Panel::Mongo { view, .. } = &tab.panel else {
        unreachable!()
    };
    assert_eq!(view.text, "{\"name\":\"中文\"}");
    assert!(view.selected.is_none());
}
