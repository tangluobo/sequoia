use super::*;

#[test]
fn sql_ctrl_drag_copies_at_the_drop_caret_and_uses_editor_undo() {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, text::CCursor, vec2};

    let ctx = egui::Context::default();
    let mut state = crate::tabs::completion::State {
        focus_requested: true,
        ..Default::default()
    };
    let original = "SELECT '中文🙂';\n-- destination";
    let mut sql = original.to_owned();
    let render = |state: &mut crate::tabs::completion::State,
                  sql: &mut String,
                  modifiers,
                  events: Vec<Event>| {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(700., 350.));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: std::iter::once(Event::ModifiersChanged(modifiers))
                    .chain(events)
                    .collect(),
                ..Default::default()
            },
            |ui| layout::editor(ui, rect, sql, state, "copy-drag-query", true),
        );
        assert!(output.platform_output.commands.is_empty());
        output.textures_delta.clear();
    };
    render(&mut state, &mut sql, Modifiers::NONE, vec![]);
    state.editor.select(&ctx, 8..11);
    render(&mut state, &mut sql, Modifiers::NONE, vec![]);
    let point = |index| {
        state.galley_pos
            + state
                .galley
                .as_ref()
                .unwrap()
                .pos_from_cursor(CCursor::new(index))
                .center()
                .to_vec2()
    };
    let source = point(9);
    let target = point(original.chars().count());
    let button = |pos, pressed, modifiers| Event::PointerButton {
        pos,
        pressed,
        modifiers,
        button: PointerButton::Primary,
    };
    let ctrl = Modifiers::CTRL | Modifiers::COMMAND;

    // A Ctrl click and a gesture cancelled by releasing Ctrl keep the text intact.
    for moved in [false, true] {
        render(
            &mut state,
            &mut sql,
            ctrl,
            vec![Event::PointerMoved(source), button(source, true, ctrl)],
        );
        if moved {
            render(
                &mut state,
                &mut sql,
                ctrl,
                vec![Event::PointerMoved(target)],
            );
            assert!(state.copy_drag.as_ref().unwrap().preview.is_some());
            render(&mut state, &mut sql, Modifiers::NONE, vec![]);
        }
        let (position, mods) = if moved {
            (target, Modifiers::NONE)
        } else {
            (source, ctrl)
        };
        render(
            &mut state,
            &mut sql,
            mods,
            vec![button(position, false, mods)],
        );
        assert_eq!(sql, original);
        assert_eq!(state.selection.slice_str(&sql), "中文🙂");
        assert!(state.copy_drag.is_none());
    }

    render(
        &mut state,
        &mut sql,
        ctrl,
        vec![Event::PointerMoved(source), button(source, true, ctrl)],
    );
    render(
        &mut state,
        &mut sql,
        ctrl,
        vec![Event::PointerMoved(target)],
    );
    assert_eq!(sql, original);
    render(
        &mut state,
        &mut sql,
        ctrl,
        vec![button(target, false, ctrl)],
    );
    assert_eq!(sql, format!("{original}中文🙂"));
    assert!(state.changed);
    assert_eq!(state.selection.slice_str(&sql), "中文🙂");
    assert!(usize::from(state.selection.as_sorted_char_range().start) > 8);

    render(
        &mut state,
        &mut sql,
        ctrl,
        vec![Event::Key {
            key: Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: ctrl,
        }],
    );
    assert_eq!(sql, original);
    assert_eq!(state.selection.slice_str(&sql), "中文🙂");
}

#[test]
fn new_query_focus_waits_until_editable_and_does_not_steal_focus_back() {
    let ctx = egui::Context::default();
    let mut state = crate::tabs::completion::State {
        focus_requested: true,
        ..Default::default()
    };
    let mut sql = String::new();
    let mut other = String::new();
    let other_id = egui::Id::new("query-other-control");
    let frame = |state: &mut crate::tabs::completion::State,
                 sql: &mut String,
                 other: &mut String,
                 enabled,
                 events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(896., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    layout::editor(
                        ui,
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(896., 400.)),
                        sql,
                        state,
                        "focus-query",
                        true,
                    )
                });
                ui.place(
                    egui::Rect::from_min_size(egui::pos2(20., 450.), egui::vec2(200., 28.)),
                    egui::TextEdit::singleline(other).id(other_id),
                );
            },
        );
        output.textures_delta.clear();
    };
    frame(&mut state, &mut sql, &mut other, false, vec![]);
    assert!(state.focus_requested);
    frame(&mut state, &mut sql, &mut other, true, vec![]);
    assert!(!state.focus_requested);
    assert!(ctx.memory(|memory| memory.focused().is_some()));
    assert_eq!(state.galley.as_ref().unwrap().job.text, "");
    assert!(
        (state.galley.as_ref().unwrap().rows[0].size.y - 17.).abs() < 0.1,
        "empty SQL must have a visible first-line caret"
    );
    for pressed in [true, false] {
        let pos = egui::pos2(100., 460.);
        frame(
            &mut state,
            &mut sql,
            &mut other,
            true,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    frame(
        &mut state,
        &mut sql,
        &mut other,
        true,
        vec![egui::Event::Text("其他输入框".into())],
    );
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(other_id));
    assert_eq!(other, "其他输入框");
    assert!(sql.is_empty());
}
#[test]
fn blank_query_accepts_typing_after_click_anywhere_in_the_editor() {
    for point in [
        egui::pos2(50., 12.),
        egui::pos2(250., 80.),
        egui::pos2(700., 300.),
    ] {
        let ctx = egui::Context::default();
        let mut state = crate::tabs::completion::State::default();
        let mut sql = String::new();
        let frame = |state: &mut crate::tabs::completion::State, sql: &mut String, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(896., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(896., 400.));
                    layout::editor(ui, rect, sql, state, "new-blank-query", true);
                },
            );
            output.textures_delta.clear();
        };
        frame(&mut state, &mut sql, vec![]);
        for pressed in [true, false] {
            frame(
                &mut state,
                &mut sql,
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
        assert!(
            ctx.memory(|memory| memory.focused().is_some()),
            "blank query has no focus at {point:?}"
        );
        frame(
            &mut state,
            &mut sql,
            vec![egui::Event::Text("SELECT '中文';".into())],
        );
        assert_eq!(
            sql, "SELECT '中文';",
            "blank query cannot type at {point:?}"
        );
    }
}
#[test]
fn editor_scrolls_both_axes_and_divider_preserves_the_live_document() {
    let ctx = egui::Context::default();
    let mut state = crate::tabs::completion::State::default();
    let mut sql = (1..=500)
        .map(|n| {
            format!(
                "SELECT {n}, '{}' FROM source_table;",
                "wide_column_".repeat(20)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let original = sql.clone();
    let mut split = None;
    let render = |state: &mut crate::tabs::completion::State,
                  sql: &mut String,
                  split: &mut Option<f32>,
                  events: Vec<egui::Event>| {
        let mut bounds = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(896., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                bounds = layout::split(ui, ui.max_rect(), split, egui::Id::new("measured-divider"));
                layout::editor(ui, bounds.0, sql, state, "scrolling", true);
            },
        );
        let numbers = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    text.galley.job.text.parse::<usize>().ok()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        output.textures_delta.clear();
        (bounds, numbers)
    };
    let (bounds, numbers) = render(&mut state, &mut sql, &mut split, vec![]);
    assert!(numbers.contains(&1));
    assert!(!numbers.contains(&500));
    let origin = state.galley_pos;
    for _ in 0..4 {
        render(
            &mut state,
            &mut sql,
            &mut split,
            vec![
                egui::Event::PointerMoved(bounds.0.center()),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(-150., -120.),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(state.galley_pos.x < origin.x && state.galley_pos.y < origin.y);
    let divider = egui::pos2(bounds.0.center().x, bounds.0.bottom() + 0.5);
    render(
        &mut state,
        &mut sql,
        &mut split,
        vec![egui::Event::PointerMoved(divider)],
    );
    render(
        &mut state,
        &mut sql,
        &mut split,
        vec![egui::Event::PointerButton {
            pos: divider,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let end = divider + egui::vec2(0., -100.);
    render(
        &mut state,
        &mut sql,
        &mut split,
        vec![egui::Event::PointerMoved(end)],
    );
    render(
        &mut state,
        &mut sql,
        &mut split,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(split.unwrap() < 0.5);
    assert_eq!(sql, original);
}
