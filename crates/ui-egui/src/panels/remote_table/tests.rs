use super::*;

#[test]
fn actual_multiple_rows_support_control_shift_select_all_and_secondary_click() {
    let ctx = egui::Context::default();
    let rows: Vec<_> = (0..8)
        .map(|i| serde_json::json!({"group":format!("group-{i}")}))
        .collect();
    let mut selection = Selection::default();
    let mut active = None;
    let mut frame = |mut events: Vec<egui::Event>, modifiers| {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(600., 500.),
                )),
                events,
                time: Some(ctx.input(|i| i.time) + 0.15),
                ..Default::default()
            },
            |ui| {
                multi_table(
                    ui,
                    &rows,
                    &mut active,
                    &[("消费者组", "group", 500.)],
                    30.,
                    &["删除消费者组"],
                    &mut selection,
                );
            },
        );
        output.textures_delta.clear();
        selection.rows.clone()
    };
    frame(vec![], egui::Modifiers::NONE);
    let visible = ctx
        .data(|d| {
            d.get_temp::<Vec<(usize, egui::Rect)>>(egui::Id::new("tomato-remote-visible-rows"))
        })
        .unwrap();
    let mut click = |index: usize, modifiers, button| {
        let pos = visible
            .iter()
            .find(|(i, _)| *i == index)
            .unwrap()
            .1
            .center();
        frame(vec![egui::Event::PointerMoved(pos)], modifiers);
        frame(
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers,
            }],
            modifiers,
        );
        frame(
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers,
            }],
            modifiers,
        )
    };
    assert_eq!(
        click(1, egui::Modifiers::NONE, egui::PointerButton::Primary),
        [1].into()
    );
    assert_eq!(
        click(3, egui::Modifiers::CTRL, egui::PointerButton::Primary),
        [1, 3].into()
    );
    assert_eq!(
        click(5, egui::Modifiers::SHIFT, egui::PointerButton::Primary),
        [3, 4, 5].into()
    );
    let selected = frame(
        vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        egui::Modifiers::COMMAND,
    );
    assert_eq!(selected, (0..8).collect());
    let pos = visible[4].1.center();
    for pressed in [true, false] {
        assert_eq!(
            frame(
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE
                    }
                ],
                egui::Modifiers::NONE
            ),
            (0..8).collect()
        );
    }
}
