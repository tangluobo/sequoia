use super::*;
fn frame(
    ctx: &egui::Context,
    inspector: &mut Inspector,
    mut events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<ManagementAction>) {
    let mut action = None;
    let mut error = None;
    let mut message = None;
    let modifiers = events
        .iter()
        .find_map(|event| match event {
            egui::Event::Key { modifiers, .. } | egui::Event::PointerButton { modifiers, .. } => {
                Some(*modifiers)
            }
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
            body(ui, inspector, false, &mut action, &mut message, &mut error);
        },
    );
    output.textures_delta.clear();
    assert!(error.is_none(), "{error:?}");
    (output, action)
}
fn point(output: &egui::FullOutput, text: &str, index: usize) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == text).then_some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        })
        .nth(index)
        .unwrap_or_else(|| panic!("Missing {text} #{index}"))
}
fn click(
    ctx: &egui::Context,
    inspector: &mut Inspector,
    p: egui::Pos2,
) -> Option<ManagementAction> {
    let event = |pressed| {
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    };
    frame(ctx, inspector, event(true));
    frame(ctx, inspector, event(false)).1
}
#[test]
fn real_resource_editor_preserves_blank_fields_and_queues_swap_adjustment() {
    let ctx = egui::Context::default();
    let mut inspector = Inspector::new(
        "web".into(),
        serde_json::json!({"HostConfig":{"Memory":536870912,"MemorySwap":1073741824}}),
    );
    let (out, _) = frame(&ctx, &mut inspector, vec![]);
    let target = point(&out, "512m", 0);
    click(&ctx, &mut inspector, target);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    frame(
        &ctx,
        &mut inspector,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            egui::Event::Text("2g".into()),
        ],
    );
    let (out, _) = frame(&ctx, &mut inspector, vec![]);
    assert_eq!(inspector.memory, "2g");
    let action = click(&ctx, &mut inspector, point(&out, "应用", 1))
        .expect("Actual resource button did not dispatch");
    let command = management::command(&action).unwrap();
    assert!(command.contains("--memory=2147483648 --memory-swap=2684354560"));
    assert!(!command.contains("--cpus="));
    inspector.policy = "on-failure".into();
    inspector.retries = 3;
    let (out, _) = frame(&ctx, &mut inspector, vec![]);
    let action = click(&ctx, &mut inspector, point(&out, "应用", 0)).unwrap();
    assert!(
        management::command(&action)
            .unwrap()
            .contains("--restart='on-failure:3'")
    );
}
