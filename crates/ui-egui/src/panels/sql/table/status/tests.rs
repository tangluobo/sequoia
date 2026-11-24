use super::*;

fn state() -> Workbench {
    let definition: TableDefinition = serde_json::from_value(serde_json::json!({
        "table":{"schema":"main","name":"复制 SQL"},"view":false,"transactional":true,
        "columns":[{"name":"order","data_type":"TEXT","nullable":true,"default":null,
            "primary_key":0,"generated":false,"auto_increment":false,"binary":false,
            "type_schema":"","type_name":"TEXT"}]
    }))
    .unwrap();
    let sql = "SELECT \"order\" FROM \"main\".\"复制 SQL\" LIMIT 1000 OFFSET 0".to_owned();
    Workbench {
        table: Some(definition.table.clone()),
        definition: Some(definition.clone()),
        page: Some(TablePage {
            definition,
            rows: vec![],
            total: 0,
            offset: 0,
            limit: 1000,
            truncated: false,
            query: Default::default(),
            sql,
        }),
        ..Default::default()
    }
}
fn frame(ctx: &egui::Context, state: &Workbench, mut events: Vec<egui::Event>) -> egui::FullOutput {
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
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700., 100.),
            )),
            events,
            time: Some(ctx.input(|i| i.time) + 0.1),
            ..Default::default()
        },
        |ui| render(ui, state, &tomato_core::Connection::new("SQLITE")),
    );
    out.textures_delta.clear();
    out
}
fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
fn copied(out: &egui::FullOutput) -> Option<&str> {
    out.platform_output.commands.iter().find_map(|cmd| {
        if let egui::OutputCommand::CopyText(text) = cmd {
            Some(text.as_str())
        } else {
            None
        }
    })
}
fn expected(state: &Workbench) -> String {
    state.page.as_ref().unwrap().sql.clone()
}
#[test]
fn sql_preview_keyboard_selection_copies_and_cannot_edit_the_statement() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let state = state();
    frame(&ctx, &state, vec![]);
    for pressed in [true, false] {
        frame(
            &ctx,
            &state,
            pointer(egui::pos2(30., 17.), egui::PointerButton::Primary, pressed),
        );
    }
    let select = egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    };
    frame(&ctx, &state, vec![select.clone()]);
    let out = frame(&ctx, &state, vec![egui::Event::Copy]);
    assert_eq!(copied(&out), Some(expected(&state).as_str()));
    frame(
        &ctx,
        &state,
        vec![egui::Event::Text("DROP TABLE anything;".into())],
    );
    frame(&ctx, &state, vec![select]);
    let out = frame(&ctx, &state, vec![egui::Event::Copy]);
    assert_eq!(copied(&out), Some(expected(&state).as_str()));
}
#[test]
fn sql_preview_context_menu_copies_full_query_even_when_showing_a_status_message() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut state = state();
    state.status = "已保存".into();
    state.query.search = "not yet executed".into();
    state.queue_query(TableQuery {
        search: "another pending query".into(),
        ..Default::default()
    });
    frame(&ctx, &state, vec![]);
    for pressed in [true, false] {
        frame(
            &ctx,
            &state,
            pointer(
                egui::pos2(30., 17.),
                egui::PointerButton::Secondary,
                pressed,
            ),
        );
    }
    let out = frame(&ctx, &state, vec![]);
    let position = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "复制 SQL").then_some(text.pos + egui::vec2(4., 5.))
            } else {
                None
            }
        })
        .expect("actual copy action");
    let mut got = None;
    for pressed in [true, false] {
        let out = frame(
            &ctx,
            &state,
            pointer(position, egui::PointerButton::Primary, pressed),
        );
        got = got.or_else(|| copied(&out).map(str::to_owned));
    }
    if got.is_none() {
        got = copied(&frame(&ctx, &state, vec![])).map(str::to_owned);
    }
    assert_eq!(got, Some(expected(&state)));
    assert_eq!(state.status, "已保存");
}
