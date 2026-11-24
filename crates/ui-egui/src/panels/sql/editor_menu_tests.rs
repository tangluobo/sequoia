use super::*;
#[test]
fn actual_sql_context_menu_preserves_selection_requests_ai_and_copies_without_os_clipboard() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut state = crate::tabs::completion::State::default();
    let mut text = "SELECT 1;\n-- ai: 人数".to_owned();
    let frame = |text: &mut String, state: &mut crate::tabs::completion::State, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                events,
                ..Default::default()
            },
            |ui| completion::render(ui, text, state, "menu-fixture", true),
        );
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some((text.galley.job.text.clone(), text.pos + egui::vec2(3., 5.)))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let copied = output.platform_output.commands.iter().find_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text.clone())
            } else {
                None
            }
        });
        output.textures_delta.clear();
        (labels, copied)
    };
    let pointer = |position, button, pressed| {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button,
                pressed,
                modifiers: Default::default(),
            },
        ]
    };
    frame(&mut text, &mut state, vec![]);
    state.editor.select(&ctx, 0..8);
    frame(&mut text, &mut state, vec![]);
    for action in ["AI 生成 SQL", "复制"] {
        let position = egui::pos2(20., 10.);
        for pressed in [true, false] {
            frame(
                &mut text,
                &mut state,
                pointer(position, egui::PointerButton::Secondary, pressed),
            );
        }
        let labels = frame(&mut text, &mut state, vec![]).0;
        for expected in ["运行已选择", "AI 生成 SQL", "剪切", "复制", "粘贴", "全选"]
        {
            assert!(
                labels.iter().any(|(label, _)| label == expected),
                "missing {expected}"
            );
        }
        let point = labels.iter().find(|(label, _)| label == action).unwrap().1;
        for pressed in [true, false] {
            frame(
                &mut text,
                &mut state,
                pointer(point, egui::PointerButton::Primary, pressed),
            );
        }
        let (_, copied) = frame(&mut text, &mut state, vec![]);
        if action == "AI 生成 SQL" {
            assert_eq!(state.ai_requested.take(), Some(true));
        } else {
            assert_eq!(copied.as_deref(), Some("SELECT 1"));
        }
        assert_eq!(
            state.selection.as_sorted_char_range(),
            egui::text::CharIndex(0)..egui::text::CharIndex(8)
        );
        assert_eq!(text, "SELECT 1;\n-- ai: 人数");
    }
}
