use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
fn frame(ctx: &egui::Context, tab: &mut Tab, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200., 800.))),
            events,
            time: Some(ctx.input(|i| i.time) + 0.04),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let intent = render(ui, tab, &mut crate::icons::Icons::default());
                assert!(intent.is_none(), "body gestures must not contact a broker");
            });
        },
    );
    out.textures_delta.clear();
    out
}
fn point(out: &egui::FullOutput, label: &str) -> Pos2 {
    out.shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape
                && t.galley.job.text == label
            {
                Some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("missing label {label}"))
}
fn click(ctx: &egui::Context, tab: &mut Tab, pos: Pos2) {
    frame(ctx, tab, vec![Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            ctx,
            tab,
            vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }],
        );
    }
}
fn view(tab: &mut Tab) -> &mut crate::tabs::mq::State {
    let Panel::MessageQueue { view, .. } = &mut tab.panel else {
        panic!("MQ")
    };
    view
}
#[test]
fn json_body_controls_both_mq_clients_open_format_fold_copy_and_clear() -> anyhow::Result<()> {
    for kind in ["ROCKETMQ", "KAFKA"] {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            egui::FontFamily::Name("java-ui".into()),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        crate::theme::install(&ctx);
        let mut tab = crate::tabs::create_tab(tomato_core::Connection::new(kind), None)?;
        let v = view(&mut tab);
        v.loaded = true;
        v.page = Page::Messages;
        v.topic = "owned".into();
        v.rows = vec![
            serde_json::json!({"msgId":"owned-message","partition":0,"key":"fixture-key","keys":"fixture-key","body":"{\"nullable\":null,\"empty\":[],\"html\":\"<tag>\"}"}),
        ];
        frame(&ctx, &mut tab, vec![]);
        let out = frame(&ctx, &mut tab, vec![]);
        let row = point(&out, "fixture-key");
        click(&ctx, &mut tab, row);
        assert!(
            view(&mut tab).body.is_empty(),
            "Java opens details on double click"
        );
        click(&ctx, &mut tab, row);
        let v = view(&mut tab);
        assert!(v.body_container && v.body.contains('\n'));
        assert!(v.body.contains("\"nullable\": null") && v.body.contains("<tag>"));
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "折叠全部"));
        let expected = view(&mut tab).copy_body();
        assert_eq!(expected, "{ /* 3 fields */ }");
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "复制全部"));
        // Copy is emitted on mouse release; inspect a second direct click result.
        let out = frame(&ctx, &mut tab, vec![]);
        let pos = point(&out, "复制全部");
        frame(
            &ctx,
            &mut tab,
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
        let out = frame(
            &ctx,
            &mut tab,
            vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(
            out.platform_output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(s)if s==&expected))
        );
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "展开全部"));
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "压缩"));
        assert!(!view(&mut tab).body.contains('\n'));
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "展开全部"));
        assert!(view(&mut tab).body.contains('\n'));
        assert!(view(&mut tab).copy_body().contains("nullable"));
        view(&mut tab).set_body("/* original Gson */ {key:'中文',missing:null,n:1.2300}".into());
        assert!(view(&mut tab).body_container);
        assert!(view(&mut tab).body.contains("\"missing\": null"));
        assert!(view(&mut tab).body.contains("1.2300"));
        view(&mut tab).set_body("{s:'a\rb'}".into());
        assert!(view(&mut tab).body.contains("a\\rb"));
        assert!(!view(&mut tab).body.contains("a\\nb"));
        view(&mut tab).set_body("\n\nsingle\r\n\n".into());
        assert_eq!(view(&mut tab).body, "\"single\"");
        assert!(!view(&mut tab).body_container);
        view(&mut tab).set_body("\n\nplain text\r\n\n".into());
        assert!(!view(&mut tab).body_container);
        assert_eq!(view(&mut tab).copy_body(), "\n\nplain text\n\n");
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, point(&out, "格式化"));
        assert_eq!(view(&mut tab).body, "\n\nplain text\n\n");
        let v = view(&mut tab);
        v.accept(tomato_services::mq::Output {
            rows: vec![],
            detail: serde_json::Value::Null,
            message: "refreshed".into(),
        });
        assert!(v.body.is_empty() && v.body_view.visible.is_empty() && !v.body_container);
        frame(
            &ctx,
            &mut tab,
            vec![Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            }],
        );
    }
    Ok(())
}
