use super::*;
mod body;

#[test]
fn actual_batch_confirm_dispatches_selected_groups_and_partial_failure_refresh_keeps_notice()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut tab = crate::tabs::create_tab(tomato_core::Connection::new("KAFKA"), None)?;
    let Panel::MessageQueue { view, .. } = &mut tab.panel else {
        unreachable!()
    };
    view.loaded = true;
    view.page = Page::Groups;
    view.rows = vec![
        serde_json::json!({"group":"first"}),
        serde_json::json!({"group":"busy"}),
        serde_json::json!({"group":"unselected"}),
    ];
    view.group_selection.rows.extend([0, 1]);
    let mut icons = crate::icons::Icons::default();
    let mut frame = |events| {
        let mut intent = None;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                time: Some(ctx.input(|i| i.time) + 0.2),
                ..Default::default()
            },
            |ui| {
                intent = render(ui, &mut tab, &mut icons);
            },
        );
        out.textures_delta.clear();
        (out, intent)
    };
    frame(vec![]);
    let (out, _) = frame(vec![]);
    let point = |out: &egui::FullOutput, label: &str| {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap()
    };
    let pos = point(&out, "批量删除");
    frame(vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    let (out, intent) = frame(vec![]);
    assert!(intent.is_none(), "confirmation must precede the write");
    point(&out, "确认批量删除");
    point(&out, "first");
    point(&out, "busy");
    let (_, cancelled) = frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    assert!(cancelled.is_none(), "Escape must never dispatch a deletion");
    frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Default::default(),
    }]);
    let (out, _) = frame(vec![]);
    assert!(
        !out.shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="确认批量删除"))
    );
    let pos = point(&out, "批量删除");
    frame(vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    let (out, _) = frame(vec![]);
    let pos = point(&out, "确定");
    frame(vec![egui::Event::PointerMoved(pos)]);
    frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    let (_, intent) = frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    let Some(Intent::Command(command)) = intent else {
        panic!("actual confirmation did not dispatch");
    };
    assert!(
        matches!(&*command, Command::MessageQueue {action:Action::DeleteGroups{groups},..} if groups==&["first","busy"])
    );
    let Panel::MessageQueue { view, .. } = &mut tab.panel else {
        unreachable!()
    };
    view.accept(tomato_services::mq::Output {rows:vec![],message:"已删除 1 个消费者组，1 个删除失败".into(),detail:serde_json::json!({"deleted":["first"],"failed":[{"group":"busy","error":"仍在使用"}]})});
    assert!(matches!(view.followup, Some(Action::Groups)));
    assert!(view.group_selection.rows.is_empty());
    view.pending = view.followup.take();
    view.accept(tomato_services::mq::Output {
        rows: vec![serde_json::json!({"group":"busy"})],
        message: String::new(),
        detail: serde_json::Value::Null,
    });
    assert_eq!(view.rows[0]["group"], "busy");
    let (title, message) = view.batch_notice.as_ref().unwrap();
    assert_eq!(title, "部分删除失败");
    assert!(message.contains("busy: 仍在使用"));
    Ok(())
}
