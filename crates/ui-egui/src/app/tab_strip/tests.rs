use super::*;

#[test]
fn overflow_captions_match_javafx_mnemonics_without_changing_main_titles() {
    for (input, expected) in [
        ("sample_db.items", "sampledb.items"),
        ("__hidden _query_more", "_hidden query_more"),
        ("连接_(连)", "连接"),
        ("连接_(💻)", "连接(💻)"),
        ("_File__Edit", "File__Edit"),
        ("file_ ", "file_ "),
        ("a_\u{a0}b", "a\u{a0}b"),
    ] {
        assert_eq!(menu_label(input), expected);
    }
}

fn header_frame(ctx: &egui::Context, tab: &Tab, events: Vec<egui::Event>) -> Header {
    let mut result = None;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900., 100.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            result = Some(header(ui, tab, &mut crate::icons::Icons::default(), true));
        },
    );
    output.textures_delta.clear();
    result.unwrap()
}
fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}

#[test]
fn original_header_geometry_and_close_slot_do_not_select_or_close_pinned_objects()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    super::super::load_fonts(&ctx);
    let mut tab = tabs::create_tab(Connection::new("SPICE"), None)?;
    tab.title = "生产服务器 SSH".into();
    let initial = header_frame(&ctx, &tab, vec![]);
    assert_eq!(initial.response.rect.height(), 37.);
    if cfg!(windows) && std::path::Path::new("C:/Windows/Fonts/msyh.ttc").exists() {
        assert_eq!(initial.response.rect.width(), 128.);
    }
    let point = initial.response.rect.right_top() + egui::vec2(-21., 18.);
    header_frame(&ctx, &tab, pointer(point, true));
    let clicked = header_frame(&ctx, &tab, pointer(point, false));
    assert!(clicked.close);
    assert!(!clicked.response.clicked());
    let mut pinned = tabs::create_tab(Connection::new("MYSQL"), None)?;
    pinned.title = "对象".into();
    if let Panel::Sql { workbench, .. } = &mut pinned.panel {
        workbench.browser = Some(Box::default());
    }
    let initial = header_frame(&ctx, &pinned, vec![]);
    if cfg!(windows) && std::path::Path::new("C:/Windows/Fonts/msyh.ttc").exists() {
        assert_eq!(initial.response.rect.width(), 71.);
    }
    let point = initial.response.rect.center();
    header_frame(&ctx, &pinned, pointer(point, true));
    let clicked = header_frame(&ctx, &pinned, pointer(point, false));
    assert!(!clicked.close);
    assert!(clicked.response.clicked());
    Ok(())
}

#[test]
fn overflow_menu_selects_hidden_tab_and_scrolls_it_into_view() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    for index in 0..8 {
        let mut tab = tabs::create_tab(Connection::new("SPICE"), None)?;
        tab.title = format!("connection {index:02} extended title");
        app.tabs.push(tab);
    }
    let selected = app.tabs[7].id.clone();
    fn frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
        let ctx = app.context.clone();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 300.),
                )),
                events,
                ..Default::default()
            },
            |ui| app.workspace_view(ui, true),
        );
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    let center = text.pos + text.galley.size() * 0.5;
                    shape
                        .clip_rect
                        .contains(center)
                        .then(|| (text.galley.job.text.clone(), center))
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
        labels
    }
    frame(&mut app, vec![]);
    let arrow = egui::pos2(391., 18.);
    for pressed in [true, false] {
        frame(&mut app, pointer(arrow, pressed));
    }
    let labels = frame(&mut app, vec![]);
    let target = labels
        .into_iter()
        .find(|(label, _)| label == "connection 07 extended title")
        .ok_or_else(|| anyhow::anyhow!("hidden tab missing from overflow menu"))?
        .1;
    for pressed in [true, false] {
        frame(&mut app, pointer(target, pressed));
    }
    assert_eq!(app.active.as_ref(), Some(&selected));
    let mut visible = false;
    for _ in 0..20 {
        visible |= frame(&mut app, vec![])
            .iter()
            .any(|(label, point)| label == "connection 07 extended title" && point.x < 382.);
    }
    assert!(
        visible,
        "selected hidden tab must scroll into the header viewport"
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
