use super::*;
#[test]
fn both_original_format_actions_replace_the_real_editor_with_one_undo_step() -> anyhow::Result<()> {
    for action in ["创建查询工具", "美化"] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tab = crate::tabs::create_tab(tomato_core::Connection::new("SQLITE"), None)?;
        let original = "select '中文' from users where id=7";
        if let Panel::Sql { sql, workbench, .. } = &mut tab.panel {
            *sql = original.into();
            workbench.schema = "main".into();
        }
        let render = |tab: &mut Tab, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::render(ui, tab);
                },
            );
            let points = output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        Some((text.galley.job.text.clone(), text.pos + egui::vec2(4., 6.)))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            output.textures_delta.clear();
            points
        };
        let points = render(&mut tab, vec![]);
        let point = points.iter().find(|(text, _)| text == action).unwrap().1;
        for pressed in [true, false] {
            render(
                &mut tab,
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
        let expected = tomato_core::sql_format::format(original)?;
        assert!(
            matches!(&tab.panel,Panel::Sql{sql,workbench,..}if sql==&expected&&workbench.document.as_ref().is_some_and(|document|document.dirty()))
        );
        let points = render(&mut tab, vec![]);
        let point = points
            .iter()
            .find(|(text, _)| text == &expected)
            .expect("formatted editor")
            .1;
        for pressed in [true, false] {
            render(
                &mut tab,
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
        render(
            &mut tab,
            vec![egui::Event::Key {
                key: egui::Key::Z,
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
        let Panel::Sql { sql, .. } = &tab.panel else {
            unreachable!()
        };
        assert_eq!(sql, original, "{action} undo must restore original SQL");
        render(
            &mut tab,
            vec![egui::Event::Key {
                key: egui::Key::Y,
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
        let Panel::Sql { sql, .. } = &tab.panel else {
            unreachable!()
        };
        assert_eq!(sql, &expected, "{action} redo must restore formatted SQL");
    }
    Ok(())
}
