use super::*;
use egui::{Event, Modifiers, PointerButton};
struct Fixture {
    ctx: egui::Context,
    view: JsonView,
    text: String,
    time: f64,
    tree: bool,
}
impl Fixture {
    fn new(text: String, tree: bool) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut view = JsonView::default();
        view.reset(&text, tree);
        let mut result = Self {
            ctx,
            view,
            text,
            time: 0.,
            tree,
        };
        result.frame(vec![]);
        result.frame(vec![]);
        result
    }
    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.time += 0.02;
        let mut out = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480., 260.))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        ui.allocate_ui(egui::vec2(470., 130.), |ui| {
                            viewer(ui, &self.text, &mut self.view, self.tree, &mut false);
                        });
                        ui.label("下方其他内容");
                    });
            },
        );
        out.textures_delta.clear();
        out
    }
    fn press(&mut self, pos: Pos2) {
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
    }
    fn release(&mut self, pos: Pos2) {
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    fn key(&mut self, key: Key, modifiers: Modifiers) -> egui::FullOutput {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }])
    }
}
fn copied(out: &egui::FullOutput) -> Option<&str> {
    out.platform_output.commands.iter().find_map(|c| {
        if let egui::OutputCommand::CopyText(s) = c {
            Some(s.as_str())
        } else {
            None
        }
    })
}
#[test]
fn json_readonly_word_movement_matches_the_two_original_control_kinds() {
    for (rich, right) in [(true, 5), (false, 6)] {
        let mut f = Fixture::new("hello world\nx\nlast".into(), rich);
        let v = f.view.viewport.unwrap();
        let pos = v.origin + egui::vec2(v.gutter + 0.1, v.line_height / 2.);
        f.press(pos);
        f.release(pos);
        f.key(Key::Home, Modifiers::CTRL);
        f.key(Key::ArrowRight, Modifiers::NONE);
        f.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(f.view.selection.head.byte, 2);
        f.key(Key::ArrowRight, Modifiers::CTRL);
        assert_eq!(f.view.selection.head.byte, right);
        f.key(Key::ArrowRight, Modifiers::NONE);
        f.key(Key::ArrowRight, Modifiers::NONE);
        f.key(Key::ArrowLeft, Modifiers::CTRL);
        assert_eq!(f.view.selection.head.byte, 6);
    }
}
#[test]
fn json_readonly_raw_copy_preserves_leading_and_trailing_empty_lines() {
    let mut f = Fixture::new("\n\n中文🍅\n\n".into(), false);
    let pos = egui::pos2(8., 7.);
    f.press(pos);
    f.release(pos);
    f.key(Key::A, Modifiers::CTRL);
    let out = f.frame(vec![Event::Copy]);
    assert_eq!(copied(&out), Some(f.text.as_str()));
    assert_eq!(f.view.visible_text(&f.text), f.text);
}
#[test]
fn json_readonly_focus_keeps_copy_and_edit_keys_out_of_the_input_editor() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut tool = ToolState::new("json_tool");
    tool.input = "INPUT_DRAFT_MUST_STAY".into();
    tool.set_json_tree("{\n  \"value\": \"READONLY_SENTINEL 中文\"\n}".into());
    let frame = |tool: &mut ToolState, events| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1100., 650.))),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    crate::panels::json::render(
                        ui,
                        tool,
                        false,
                        &mut crate::icons::Icons::default(),
                    );
                });
            },
        );
        out.textures_delta.clear();
        out
    };
    frame(&mut tool, vec![]);
    frame(&mut tool, vec![]);
    let v = tool.json_view.viewport.unwrap();
    let pos = v.origin + egui::vec2(v.gutter + 0.1, v.line_height / 2.);
    frame(
        &mut tool,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    frame(
        &mut tool,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    frame(
        &mut tool,
        vec![Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        }],
    );
    let out = frame(&mut tool, vec![Event::Copy]);
    assert_eq!(copied(&out), Some(tool.output.as_str()));
    frame(
        &mut tool,
        vec![
            Event::Text("should not edit".into()),
            Event::Paste("should not paste".into()),
            Event::Key {
                key: Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert_eq!(tool.input, "INPUT_DRAFT_MUST_STAY");
    assert!(tool.output.contains("READONLY_SENTINEL"));
}
#[test]
fn json_readonly_drag_autoscroll_and_keyboard_copy_include_offscreen_rows() {
    let text = (0..250)
        .map(|n| format!("行{n:03} 中文🍅 emoji 👩‍💻"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut f = Fixture::new(text, false);
    let start = egui::pos2(5., 7.);
    f.press(start);
    let end = egui::pos2(110., 195.);
    for _ in 0..30 {
        f.frame(vec![Event::PointerMoved(end)]);
    }
    f.release(end);
    let range = f.view.selection.range().expect("drag must select text");
    assert_eq!(range.0, Point::default());
    assert!(
        range.1.row > 12,
        "drag must scroll beyond first viewport: {range:?}"
    );
    let out = f.frame(vec![Event::Copy]);
    let expected = f.view.selected_text(&f.text, &f.view.selection).unwrap();
    assert_eq!(copied(&out), Some(expected.as_str()));
    assert!(expected.contains("行010"));
    f.key(Key::A, Modifiers::CTRL);
    let out = f.key(Key::C, Modifiers::CTRL);
    assert!(
        copied(&out) == Some(&f.text),
        "Ctrl+A/C must copy every offscreen row"
    );
    let out = f.frame(vec![]);
    let shapes = out
        .shapes
        .iter()
        .filter(|s| matches!(&s.shape, egui::Shape::Text(_)))
        .count();
    assert!(
        shapes < 30,
        "selection must retain visible-row rendering, got {shapes}"
    );
}
#[test]
fn json_readonly_megabyte_line_selection_copy_and_grapheme_navigation_stay_bounded() {
    let text = format!("start {} end", "中文👩‍💻e\u{301}".repeat(60_000));
    let mut f = Fixture::new(text, false);
    let pos = egui::pos2(8., 7.);
    f.press(pos);
    f.release(pos);
    f.key(Key::End, Modifiers::CTRL);
    f.key(Key::Home, Modifiers::SHIFT);
    let out = f.frame(vec![Event::Copy]);
    assert!(
        copied(&out) == Some(&f.text),
        "the entire horizontally clipped line must be copyable"
    );
    f.key(Key::ArrowRight, Modifiers::NONE);
    f.key(Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(
        f.view.selected_text(&f.text, &f.view.selection).as_deref(),
        Some("d")
    );
    let out = f.frame(vec![]);
    assert!(
        out.shapes
            .iter()
            .all(|s| !matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.len()>4096)),
        "never shape a full megabyte line"
    );
    assert!(f.view.long_lines.len() <= 128);
}
#[test]
fn json_readonly_folding_resets_selection_and_copy_uses_visible_summary() {
    let text = "{\n  \"nested\": [\n    \"中文\",\n    \"👩‍💻\"\n  ],\n  \"ok\": true\n}".to_owned();
    let mut f = Fixture::new(text, true);
    let pos = egui::pos2(80., 7.);
    f.press(pos);
    f.release(pos);
    f.key(Key::A, Modifiers::CTRL);
    let vp = f.view.viewport.unwrap();
    let arrow = egui::pos2(
        vp.rect.left() - vp.gutter + 48.,
        vp.origin.y + vp.line_height * 1.5,
    );
    f.press(arrow);
    f.release(arrow);
    assert!(f.view.selection.range().is_none());
    f.frame(vec![]);
    f.key(Key::A, Modifiers::CTRL);
    let out = f.frame(vec![Event::Copy]);
    assert_eq!(copied(&out), Some(f.view.visible_text(&f.text).as_str()));
    assert!(copied(&out).unwrap().contains("/* 2 items */"));
    let vp = f.view.viewport.unwrap();
    let node = egui::pos2(
        vp.rect.left() - vp.gutter + 64.,
        vp.origin.y + vp.line_height * 1.5,
    );
    f.press(node);
    let out = f.frame(vec![Event::PointerButton {
        pos: node,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    let value: serde_json::Value =
        serde_json::from_str(copied(&out).expect("original node copy graphic")).unwrap();
    assert_eq!(value, serde_json::json!(["中文", "👩‍💻"]));
    f.view.reset("replacement", false);
    assert!(f.view.selection.range().is_none());
}
