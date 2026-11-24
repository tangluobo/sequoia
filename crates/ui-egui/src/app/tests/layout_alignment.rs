use super::*;

fn frame(app: &mut TomatoApp, width: f32, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let time = ctx.input(|input| input.time) + 0.1;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 900.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}

#[test]
fn mq_values_use_remaining_width_and_offset_copy_keeps_the_complete_value() -> anyhow::Result<()> {
    for kind in ["KAFKA", "ROCKETMQ"] {
        let root = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(root.path().join("connections.json")),
        );
        let mut tab = tabs::create_tab(Connection::new(kind), None)?;
        let Panel::MessageQueue { view, .. } = &mut tab.panel else {
            unreachable!()
        };
        view.loaded = true;
        view.page = tabs::mq::Page::Messages;
        view.topic = "orders.production".into();
        let id = "7F00000100002A9F00000000000004D2";
        view.detail = serde_json::json!({"msgId":id,"offset":1234567890,"key":"customer-order-20261020-1234567890","headers":[{"key":"来源","value":"生产环境"},{"key":"trace-id","value":id}]}).to_string();
        app.active = Some(tab.id.clone());
        app.tabs.push(tab);
        for width in [1200., 900., 1600., 1200.] {
            for _ in 0..4 {
                frame(&mut app, width, vec![]);
            }
            let output = frame(&mut app, width, vec![]);
            let texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let value = texts
                .iter()
                .find(|text| text.galley.job.text == id)
                .expect("actual message ID");
            let offset = texts
                .iter()
                .find(|text| text.galley.job.text == "1234567890")
                .expect("actual offset");
            let offset_label = texts
                .iter()
                .find(|text| text.galley.job.text == "offset:")
                .expect("offset label");
            assert!(
                ((offset_label.pos.y + offset_label.galley.size().y / 2.)
                    - (offset.pos.y + offset.galley.size().y / 2.))
                    .abs()
                    <= 1.,
                "key and value centerlines"
            );
            assert!(
                (value.pos.x - offset.pos.x).abs() <= 1.,
                "{kind} at {width}: value column alignment: ID {:?}, offset {:?}",
                value.pos,
                offset.pos
            );
            assert!(
                value.galley.rows.len() <= if width == 900. { 3 } else { 2 },
                "{kind} at {width}: {} rows in {} px",
                value.galley.rows.len(),
                value.galley.job.wrap.max_width
            );
            assert!(
                value.galley.job.wrap.max_width >= if width == 900. { 90. } else { 180. },
                "value cell was squeezed: {}",
                value.galley.job.wrap.max_width
            );
            let headers = texts
                .iter()
                .find(|text| text.galley.job.text.starts_with("来源=生产环境\ntrace-id="))
                .expect("explicit header newlines preserved");
            assert_eq!(headers.pos.x, offset.pos.x);
            // The offset copy button is the last 18 px of the measured key
            // column, immediately before the 8 px value gap.
            let point = egui::pos2(offset.pos.x - 17., offset.pos.y + 8.);
            let mut copied = false;
            for pressed in [true, false] {
                let output = frame(
                    &mut app,
                    width,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
                copied |= output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(value) if value == "1234567890"));
            }
            assert!(
                copied,
                "actual offset button must copy the unwrapped original"
            );
        }
        assert!(app.pending.is_empty());
    }
    Ok(())
}

#[test]
fn connection_tree_uses_one_font_and_centerline_for_chinese_and_mixed_names() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(root.path().join("connections.json")),
    );
    for (kind, name) in [
        ("LOCAL_TERMINAL", "本地终端"),
        ("SSH", "SSH 生产服务器"),
        ("MYSQL", "MySQL 数据库"),
    ] {
        let mut connection = Connection::new(kind);
        connection.name = name.into();
        app.store.connections.push(connection);
    }
    for _ in 0..4 {
        frame(&mut app, 1200., vec![]);
    }
    let output = frame(&mut app, 1200., vec![]);
    let mut positions = vec![];
    for name in ["本地终端", "SSH 生产服务器", "MySQL 数据库"] {
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == name => Some(text),
                _ => None,
            })
            .expect("tree label");
        assert_eq!(
            text.galley.job.sections[0].format.font_id,
            crate::theme::java_font(12.)
        );
        positions.push(text.pos + text.galley.size() / 2.);
    }
    for pair in positions.windows(2) {
        assert!(
            ((pair[1].y - pair[0].y).abs() - crate::theme::TREE_ROW_HEIGHT).abs() <= 1.,
            "row centerlines: {positions:?}"
        );
    }
    if cfg!(windows) && std::path::Path::new("C:/Windows/Fonts/msyh.ttc").exists() {
        ctx.fonts_mut(|fonts| {
            let ordinary = fonts.layout_no_wrap(
                "本地终端 SSH 123".into(),
                egui::FontId::proportional(13.),
                egui::Color32::BLACK,
            );
            let native = fonts.layout_no_wrap(
                "本地终端 SSH 123".into(),
                crate::theme::java_font(13.),
                egui::Color32::BLACK,
            );
            assert_eq!(
                ordinary.size(),
                native.size(),
                "ordinary labels and native labels must share line metrics"
            );
            for (a, b) in ordinary.rows[0].glyphs.iter().zip(&native.rows[0].glyphs) {
                assert_eq!(a.pos, b.pos, "mixed CJK/Latin baseline");
            }
        });
    }
    Ok(())
}
