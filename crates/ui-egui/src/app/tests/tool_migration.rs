use super::*;

#[test]
fn converter_native_button_recurses_reports_partial_failures_and_preserves_the_config()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let before = std::fs::read(&config)?;
    let source = directory.path().join("svg/nested");
    std::fs::create_dir_all(&source)?;
    std::fs::write(
        source.join("中文.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="7"><rect width="11" height="7" fill="#1964c8"/></svg>"##,
    )?;
    std::fs::write(source.join("bad.svg"), "broken svg")?;
    let output = directory.path().join("new/png");
    let mut app = app(directory.path())?;
    app.section = 1;
    app.open_tool("image_format_converter", "图片格式转换");
    click(&mut app, "开始转换")?;
    let Panel::Tool(tool) = &mut app.tabs[0].panel else {
        panic!("tool")
    };
    assert_eq!(
        tool.converter.as_ref().unwrap().status,
        "请选择源文件或目录"
    );
    tool.source = directory.path().join("svg").display().to_string();
    tool.destination = output.display().to_string();
    click(&mut app, "开始转换")?;
    let started = std::time::Instant::now();
    while app.tabs[0].busy {
        app.logic(&app.context.clone());
        frame(&mut app, vec![]);
        anyhow::ensure!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "native conversion did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("tool")
    };
    let state = tool.converter.as_ref().unwrap();
    assert_eq!(
        state.status,
        "转换完成，有失败项！总计: 2, 成功: 1, 失败: 1"
    );
    assert!(state.error && !state.running());
    assert!(app.error.is_none() && app.tabs[0].error.is_none());
    let image = image::open(output.join("nested/中文.png"))?.to_rgba8();
    assert_eq!(image.dimensions(), (11, 7));
    assert_eq!(image.get_pixel(5, 3).0, [25, 100, 200, 255]);
    assert_eq!(std::fs::read(config)?, before);
    let final_frame = frame(&mut app, vec![]);
    assert!(final_frame.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.starts_with("转换完成，有失败项"))));
    Ok(())
}

#[test]
fn dataset_native_button_writes_nested_annotations_and_rejected_jobs_release_the_view()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let before = std::fs::read(&config)?;
    let source = directory.path().join("voc/nested");
    std::fs::create_dir_all(&source)?;
    std::fs::write(
        source.join("one.xml"),
        "<annotation><filename>中文.jpg</filename><size><width>10</width><height>8</height></size><object><name>cat</name><bndbox><xmin>1</xmin><ymin>2</ymin><xmax>9</xmax><ymax>7</ymax></bndbox></object></annotation>",
    )?;
    std::fs::write(source.join("broken.xml"), "broken")?;
    let output = directory.path().join("coco　");
    let mut app = app(directory.path())?;
    app.section = 1;
    app.open_tool("dataset_converter", "数据集格式转换");
    let Panel::Tool(tool) = &mut app.tabs[0].panel else {
        panic!("tool")
    };
    tool.source = directory.path().join("voc").display().to_string();
    tool.destination = output.display().to_string();
    click(&mut app, "开始转换")?;
    let started = std::time::Instant::now();
    while app.tabs[0].busy {
        app.logic(&app.context.clone());
        frame(&mut app, vec![]);
        anyhow::ensure!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "native dataset conversion did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("coco_annotations.json"))?)?;
    assert_eq!(data["images"][0]["file_name"], "中文.jpg");
    assert_eq!(
        data["annotations"][0]["bbox"],
        serde_json::json!([1, 2, 8, 5])
    );
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("tool")
    };
    assert_eq!(tool.converter.as_ref().unwrap().status, "转换完成！");
    assert!(!tool.converter.as_ref().unwrap().error);
    let shutdown = app.queue.shutdown();
    click(&mut app, "开始转换")?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("tool")
    };
    let state = tool.converter.as_ref().unwrap();
    assert!(state.error && !state.running());
    assert!(state.status.starts_with("转换失败：后台任务正在关闭"));
    assert!(app.error.is_none() && app.tabs[0].error.is_none());
    app.runtime.as_ref().unwrap().block_on(shutdown.wait());
    assert_eq!(std::fs::read(config)?, before);
    Ok(())
}
fn frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 750.),
            )),
            events,
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}
fn click(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    let output = frame(app, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing native control: {label}"))?;
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    Ok(())
}
fn app(directory: &std::path::Path) -> anyhow::Result<TomatoApp> {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    Ok(TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.join("connections.json")),
    ))
}
#[test]
fn connection_rows_and_content_stay_below_their_headers_while_scrolling() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = app(directory.path())?;
    for i in 0..100 {
        let mut c = Connection::new("SSH");
        c.name = format!("clip-row-{i:03}");
        app.store.connections.push(c);
    }
    app.section = 0;
    app.open_tool("desktop_shortcut", "桌面快捷方式");
    app.tabs
        .push(tabs::create_tab(Connection::new("SQLITE"), None)?);
    frame(&mut app, vec![]);
    let sidebar = app.layout.sidebar;
    let mut rows_seen = 0;
    let mut content_seen = 0;
    for _ in 0..4 {
        let output = frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(sidebar.center()),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -130.),
                    modifiers: Default::default(),
                },
            ],
        );
        let header = app.layout.header.bottom();
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                if text.galley.job.text.starts_with("clip-row-") {
                    rows_seen += 1;
                    assert!(
                        shape.clip_rect.top() >= header - 0.1,
                        "tree painted over search/header divider: {:?}",
                        shape.clip_rect
                    );
                }
                if text.galley.job.text == "应用名称 *" {
                    content_seen += 1;
                    assert!(
                        shape.clip_rect.top() >= header + crate::theme::TAB_HEIGHT - 0.1,
                        "content painted over tabs/header divider"
                    );
                }
            }
        }
    }
    assert!(rows_seen > 10 && content_seen > 0);
    Ok(())
}
#[test]
fn shortcut_native_tabs_preview_copy_and_actual_save_keep_connections_unchanged()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let original = std::fs::read(&config)?;
    let mut app = app(directory.path())?;
    app.section = 1;
    app.open_tool("desktop_shortcut", "桌面快捷方式");
    if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        let state = tool.shortcut.get_or_insert_with(Default::default);
        state.directory = directory.path().join("launchers").display().to_string();
        state.fields.name = "Fixture App".into();
        state.fields.executable = "/opt/fixture app/run".into();
        state.fields.icon = "fixture-icon".into();
        state.fields.wm_class = "Fixture".into();
        state.fields.arguments = "--new".into();
        state.fields.terminal = true;
    }
    frame(&mut app, vec![]);
    click(&mut app, "高级设置")?;
    assert!(
        frame(&mut app, vec![])
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text=="执行参数"))
    );
    click(&mut app, "预览")?;
    assert!(frame(&mut app,vec![]).shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.contains("StartupWMClass=Fixture"))));
    click(&mut app, "复制到剪贴板")?;
    click(&mut app, "创建快捷方式")?;
    let path = directory.path().join("launchers/fixture-app.desktop");
    let ctx = app.context.clone();
    let start = std::time::Instant::now();
    while !path.exists() || app.tabs[0].busy {
        app.logic(&ctx);
        frame(&mut app, vec![]);
        anyhow::ensure!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "save command timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let content = std::fs::read_to_string(path)?;
    assert!(content.contains("Terminal=true\n"));
    assert!(content.contains("Exec=\"/opt/fixture app/run\" --new\n"));
    assert_eq!(std::fs::read(config)?, original);
    Ok(())
}

#[test]
fn icon_native_preview_crop_and_save_write_the_displayed_result() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fixture.png");
    let pixels = image::RgbaImage::from_fn(100, 80, |x, y| {
        image::Rgba(if (10..90).contains(&x) && (10..70).contains(&y) {
            [40, 100, 210, 255]
        } else {
            [255, 255, 255, 255]
        })
    });
    pixels.save(&source)?;
    let mut app = app(directory.path())?;
    app.section = 1;
    app.open_tool("image_background_remover", "图标制作工具");
    if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        tool.source = source.display().to_string();
        let state = tool.icon_tool.get_or_insert_with(Default::default);
        state.invalidate();
        state.dirty_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    }
    let start = std::time::Instant::now();
    loop {
        frame(&mut app, vec![]);
        if matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.icon_tool.as_ref().is_some_and(|s|s.ready()))
        {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "icon preview timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let initial = if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        let state = tool.icon_tool.as_mut().unwrap();
        let initial = state.sizes[1];
        state.crop.square = false;
        state.crop.set_margins([2, 3, 4, 5]);
        state.sync_fields();
        state.scroll_offset = 500.;
        initial
    } else {
        unreachable!()
    };
    assert_eq!(
        initial,
        [100, 80],
        "single-file preview must retain transparent borders like Java"
    );
    frame(&mut app, vec![]);
    click(&mut app, "应用裁切")?;
    let start = std::time::Instant::now();
    loop {
        frame(&mut app, vec![]);
        if matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.icon_tool.as_ref().is_some_and(|s|s.ready()))
        {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "cropped preview timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    click(&mut app, "保存")?;
    let output = source.with_file_name("fixture_transparent.png");
    let start = std::time::Instant::now();
    loop {
        frame(&mut app, vec![]);
        if output.exists()
            && matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.icon_tool.as_ref().is_some_and(|s|s.job.is_none()))
        {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "icon save timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        image::image_dimensions(output)?,
        (initial[0] - 6, initial[1] - 8)
    );
    Ok(())
}

#[test]
fn downloader_native_settings_add_validation_download_and_remove_use_the_real_actor()
-> anyhow::Result<()> {
    downloader_controls_flow(false)
}
#[test]
fn saved_downloader_connection_settings_add_download_and_remove_use_the_real_actor()
-> anyhow::Result<()> {
    downloader_controls_flow(true)
}
fn downloader_controls_flow(saved_connection: bool) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let mut connection = Connection::new("TOOL");
    connection.name = "下载器".into();
    connection.set("toolType", "downloader");
    Store {
        connections: if saved_connection {
            vec![connection.clone()]
        } else {
            vec![]
        },
        ..Default::default()
    }
    .save(&config)?;
    let original = std::fs::read(&config)?;
    let mut app = app(directory.path())?;
    let ctx = app.context.clone();
    let bridge = super::native_bridge::Bridge::new(&ctx);
    let bytes = vec![57u8; 123456];
    let content = bytes.clone();
    let (url, server) = app.runtime.as_ref().unwrap().block_on(async {
        let router = axum::Router::new().route(
            "/fixture.bin",
            axum::routing::get(move || {
                let content = content.clone();
                async move { content }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/fixture.bin", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        Ok::<_, anyhow::Error>((url, server))
    })?;
    app.section = 1;
    if saved_connection {
        app.open_connection(connection);
    } else {
        app.open_tool("downloader", "下载器");
    }
    fn render(
        bridge: &super::native_bridge::Bridge,
        app: &mut TomatoApp,
        root: Vec<egui::Event>,
        child: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.context.clone();
        bridge.root_frame(&ctx, root, child, |ui| app.ui(ui))
    }
    fn press(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
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
    fn root_click(
        bridge: &super::native_bridge::Bridge,
        app: &mut TomatoApp,
        label: &str,
    ) -> anyhow::Result<()> {
        let output = render(bridge, app, vec![], vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() / 2.)
                }
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("missing root control: {label}"))?;
        for down in [true, false] {
            render(bridge, app, press(point, down), vec![]);
        }
        render(bridge, app, vec![], vec![]);
        Ok(())
    }
    fn child_click(
        bridge: &super::native_bridge::Bridge,
        app: &mut TomatoApp,
        label: &str,
    ) -> anyhow::Result<()> {
        let point = bridge.text(label)?;
        for down in [true, false] {
            render(bridge, app, vec![], press(point, down));
        }
        render(bridge, app, vec![], vec![]);
        Ok(())
    }
    fn state(app: &mut TomatoApp) -> &mut crate::tabs::downloader::State {
        let Panel::Tool(tool) = &mut app.tabs[0].panel else {
            panic!("expected downloader")
        };
        tool.downloader.as_mut().unwrap()
    }
    let start = std::time::Instant::now();
    while !state(&mut app).loaded_settings {
        render(&bridge, &mut app, vec![], vec![]);
        anyhow::ensure!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let before = state(&mut app).defaults.directory.clone();
    root_click(&bridge, &mut app, "设置")?;
    state(&mut app).dialog.as_mut().unwrap().directory = directory.path().display().to_string();
    child_click(&bridge, &mut app, "取消")?;
    assert!(state(&mut app).dialog.is_none());
    assert_eq!(state(&mut app).defaults.directory, before);
    root_click(&bridge, &mut app, "设置")?;
    {
        let dialog = state(&mut app).dialog.as_mut().unwrap();
        dialog.directory = directory.path().display().to_string();
        dialog.threads = "3".into();
        dialog.concurrent = "1".into();
    }
    child_click(&bridge, &mut app, "保存")?;
    assert!(state(&mut app).dialog.is_none());
    assert_eq!(state(&mut app).defaults.threads, 3);
    root_click(&bridge, &mut app, "添加下载")?;
    state(&mut app).dialog.as_mut().unwrap().url = "magnet:?xt=urn:btih:fixture".into();
    child_click(&bridge, &mut app, "添加下载")?;
    assert!(state(&mut app).dialog.as_ref().unwrap().error.is_some());
    assert!(state(&mut app).rows.is_empty());
    state(&mut app).dialog.as_mut().unwrap().url = url;
    child_click(&bridge, &mut app, "添加下载")?;
    assert!(state(&mut app).dialog.is_none());
    let start = std::time::Instant::now();
    loop {
        render(&bridge, &mut app, vec![], vec![]);
        if state(&mut app).rows.iter().any(|r| r.state == "COMPLETED") {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "native download timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(directory.path().join("fixture.bin"))?, bytes);
    root_click(&bridge, &mut app, "fixture.bin")?;
    root_click(&bridge, &mut app, "移除记录")?;
    let start = std::time::Instant::now();
    while !state(&mut app).rows.is_empty() {
        render(&bridge, &mut app, vec![], vec![]);
        anyhow::ensure!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(directory.path().join("fixture.bin").exists());
    assert_eq!(std::fs::read(config)?, original);
    server.abort();
    Ok(())
}

#[test]
#[ignore = "requires the owned loopback adb_fixture example"]
fn actual_adb_tab_draws_device_sends_click_tab_and_text_and_releases_search_focus()
-> anyhow::Result<()> {
    use std::time::{Duration, Instant};
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let original = std::fs::read(&config)?;
    let exe = adb_peer::Fixture::start(directory.path())?;
    let mut app = app(directory.path())?;
    let ctx = app.context.clone();
    let bridge = super::native_bridge::Bridge::new(&ctx);
    let mut connection = exe.connection();
    connection.name = "owned-adb".into();
    connection.set("adbDisplayMode", "PNG");
    app.open_connection(connection);
    fn draw(
        bridge: &super::native_bridge::Bridge,
        app: &mut TomatoApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.context.clone();
        bridge.root_frame(&ctx, events, vec![], |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }
    let start = Instant::now();
    let surface = loop {
        let output = draw(&bridge, &mut app, vec![]);
        let Panel::NativeDesktop { session, view, .. } = &app.tabs[0].panel else {
            panic!("ADB must use embedded desktop");
        };
        if let Some(session) = session
            && let tomato_services::remote_desktop::Status::Failed(e) = session.status()
        {
            anyhow::bail!(e);
        }
        if let Some(texture) = &view.texture
            && let Some(rect) = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() => {
                    Some(mesh.calc_bounds())
                }
                _ => None,
            })
        {
            assert_eq!(view.size, [96, 64]);
            break rect;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(15),
            "Embedded adb frame not drawn"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let point = surface.center();
    for pressed in [true, false] {
        draw(
            &bridge,
            &mut app,
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
    }
    for pressed in [true, false] {
        draw(
            &bridge,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
    }
    draw(
        &bridge,
        &mut app,
        vec![egui::Event::Text("native-typed".into())],
    );
    let log = exe.with_extension("commands.jsonl");
    let start = Instant::now();
    loop {
        draw(&bridge, &mut app, vec![]);
        let text = std::fs::read_to_string(&log)?;
        if text.contains("native-typed") && text.contains("'keyevent' '61'") {
            assert!(
                (47..=49).any(|x| (31..=33).any(|y| text.contains(&format!("'tap' '{x}' '{y}'")))),
                "Expected a center tap within one device pixel; commands: {text}"
            );
            break;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(10),
            "ADB native input not delivered: {text}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    draw(&bridge, &mut app, vec![egui::Event::Text("中".into())]);
    for _ in 0..10 {
        draw(&bridge, &mut app, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(&app.tabs[0].panel,Panel::NativeDesktop{session:Some(session),..} if session.status()==tomato_services::remote_desktop::Status::Ready)
    );
    let search = egui::pos2(
        app.layout.sidebar.left() + 80.,
        app.layout.header.bottom() + 16.,
    );
    for pressed in [true, false] {
        draw(
            &bridge,
            &mut app,
            vec![
                egui::Event::PointerMoved(search),
                egui::Event::PointerButton {
                    pos: search,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    draw(
        &bridge,
        &mut app,
        vec![egui::Event::Text("must-not-reach".into())],
    );
    for _ in 0..15 {
        draw(&bridge, &mut app, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!std::fs::read_to_string(log)?.contains("must-not-reach"));
    assert_eq!(std::fs::read(config)?, original);
    crate::enable_gpu_video(&app.context, egui_wgpu::wgpu::TextureFormat::Bgra8Unorm);
    click(&mut app, "流畅视频")?;
    let started = Instant::now();
    let surface = loop {
        let output = draw(&bridge, &mut app, vec![]);
        let Panel::NativeDesktop {
            session: Some(session),
            view,
            ..
        } = &app.tabs[0].panel
        else {
            anyhow::bail!("Missing H.264 tab")
        };
        if let tomato_services::remote_desktop::Status::Failed(error) = session.status() {
            anyhow::bail!(error);
        }
        if session.adb_video_stats().is_some_and(|s| s.mode == "H.264")
            && view.size == [64, 48]
            && view.video.is_some()
        {
            assert!(
                view.texture.is_none(),
                "GPU path retained a redundant RGBA texture"
            );
            if let Some(rect) = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Callback(callback) => Some(callback.rect),
                _ => None,
            }) {
                break rect;
            }
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(15),
            "H.264 native canvas missing"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let commands = std::fs::read_to_string(exe.with_extension("commands.jsonl"))?;
    assert!(commands.contains("max_size=1920"));
    assert!(commands.contains("video_bit_rate=24000000"));
    assert!(commands.contains("max_fps=60"));
    let point = surface.center();
    draw(
        &bridge,
        &mut app,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    draw(
        &bridge,
        &mut app,
        vec![egui::Event::PointerMoved(point + egui::vec2(20., 10.))],
    );
    let input_log = exe.with_extension("scrcpy-input.jsonl");
    let started = Instant::now();
    loop {
        draw(&bridge, &mut app, vec![]);
        let events = std::fs::read_to_string(&input_log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Vec<u8>>(l).ok())
            .collect::<Vec<_>>();
        if events.iter().any(|e| e.starts_with(&[2, 2])) {
            assert!(events.iter().any(|e| e.starts_with(&[2, 0])));
            assert!(
                !events.iter().any(|e| e.starts_with(&[2, 1])),
                "Move arrived only after release"
            );
            break;
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(5),
            "Live gesture missing before mouse release"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    draw(
        &bridge,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: point + egui::vec2(20., 10.),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    click(&mut app, "高清 1920")?;
    click(&mut app, "流畅 1024")?;
    let started = Instant::now();
    loop {
        draw(&bridge, &mut app, vec![]);
        let commands = std::fs::read_to_string(exe.with_extension("commands.jsonl"))?;
        let ready = matches!(&app.tabs[0].panel, Panel::NativeDesktop { session:Some(session), .. } if session.status()==tomato_services::remote_desktop::Status::Ready);
        if commands.contains("max_size=1024")
            && commands.contains("video_bit_rate=4000000")
            && ready
        {
            break;
        }
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(10),
            "Native quality selector did not restart actual encoder"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    app.shutdown_file_transfers();
    drop(app);
    Ok(())
}

#[test]
fn native_port_mapping_editor_blocks_root_tool_switching_until_cancel() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = app(directory.path())?;
    let ctx = app.context.clone();
    let bridge = super::native_bridge::Bridge::new(&ctx);
    app.section = 1;
    app.open_tool("port_mapping", "端口映射");
    if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        tool.port_mapping = Some(Box::new(crate::tabs::port_mapping::State::new(
            directory.path().join("port-mappings.json"),
            None,
            directory.path().join("known_hosts"),
        )));
    }
    let id = app.active.clone();
    click(&mut app, "添加")?;
    assert!(
        matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.port_mapping.as_ref().unwrap().editor.is_some())
    );
    click(&mut app, "JSON处理工具")?;
    assert_eq!(
        app.active, id,
        "Root tool switching escaped the native modal"
    );
    bridge.root_frame(
        &ctx,
        vec![],
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ui| app.ui(ui),
    );
    assert!(
        matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.port_mapping.as_ref().unwrap().editor.is_none())
    );
    click(&mut app, "JSON处理工具")?;
    assert_ne!(app.active, id);
    assert!(!directory.path().join("port-mappings.json").exists());
    Ok(())
}

#[test]
#[ignore = "requires the owned loopback TOMATO_PORT_FIXTURE_DIR server"]
fn actual_port_mapping_native_controls_create_transfer_stop_both_ssh_directions()
-> anyhow::Result<()> {
    use std::time::{Duration, Instant};
    let fixture = PathBuf::from(
        std::env::var_os("TOMATO_PORT_FIXTURE_DIR")
            .ok_or_else(|| anyhow::anyhow!("Missing owned SSH fixture"))?,
    );
    anyhow::ensure!(
        fixture
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("tomato-port-fixture-")),
        "Requires own fixture"
    );
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("ready.json"))?)?;
    let host: Connection = serde_json::from_value(ready["connection"].clone())?;
    let echo = ready["echoPort"].as_u64().unwrap() as u16;
    assert_eq!(host.text("host"), "127.0.0.1");
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    Store::default().save(&config)?;
    let original = std::fs::read(&config)?;
    let mut app = app(directory.path())?;
    let ctx = app.context.clone();
    let bridge = super::native_bridge::Bridge::new(&ctx);
    app.store.connections.push(host.clone());
    app.section = 1;
    app.open_tool("port_mapping", "端口映射");
    if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        tool.port_mapping = Some(Box::new(crate::tabs::port_mapping::State::new(
            directory.path().join("port-mappings.json"),
            app.runtime.as_ref().map(|r| r.handle().clone()),
            directory.path().join("known_hosts"),
        )));
    }
    fn state(app: &mut TomatoApp) -> &mut crate::tabs::port_mapping::State {
        let Panel::Tool(tool) = &mut app.tabs[0].panel else {
            panic!("port tool");
        };
        tool.port_mapping.as_mut().unwrap()
    }
    fn native_click(
        bridge: &super::native_bridge::Bridge,
        app: &mut TomatoApp,
        label: &str,
    ) -> anyhow::Result<()> {
        let ctx = app.context.clone();
        bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
        let pos = bridge.text(label)?;
        for pressed in [true, false] {
            bridge.root_frame(
                &ctx,
                vec![],
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                |ui| app.ui(ui),
            );
        }
        Ok(())
    }
    for direction in ["L", "R"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let forwarded = listener.local_addr()?.port();
        drop(listener);
        click(&mut app, "添加")?;
        let editor = state(&mut app).editor.as_mut().unwrap();
        editor.mapping.name = format!("native-{direction}");
        editor.mapping.ssh_host_id = host.id.clone();
        editor.mapping.direction = direction.into();
        editor.local = if direction == "L" { forwarded } else { echo }.to_string();
        editor.remote = if direction == "L" { echo } else { forwarded }.to_string();
        native_click(&bridge, &mut app, "确定")?;
        assert!(state(&mut app).editor.is_none());
        assert_eq!(state(&mut app).mappings.len(), 1);
        let output = frame(&mut app, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == format!("native-{direction}") => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Actual mapping row missing"))?;
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(state(&mut app).selected.contains(&0));
        click(&mut app, "启动")?;
        if direction == "R" {
            assert_eq!(state(&mut app).reverse.len(), 1);
            click(&mut app, "是")?;
        }
        let started = Instant::now();
        loop {
            frame(&mut app, vec![]);
            if state(&mut app)
                .active
                .values()
                .any(|forward| forward.is_running())
            {
                break;
            }
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(15),
                "Native SSH mapping did not start: {}",
                state(&mut app).status
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        app.runtime.as_ref().unwrap().block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", forwarded)).await?;
                socket.write_all(b"native-mapping-payload").await?;
                let mut bytes = [0; 22];
                socket.read_exact(&mut bytes).await?;
                assert_eq!(&bytes, b"native-mapping-payload");
                Ok::<_, anyhow::Error>(())
            })
            .await?
        })?;
        click(&mut app, "停止")?;
        assert!(state(&mut app).active.is_empty());
        let started = Instant::now();
        loop {
            if std::net::TcpListener::bind(("127.0.0.1", forwarded)).is_ok() {
                break;
            }
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(5),
                "Stopped mapping kept its listener"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        click(&mut app, "删除")?;
        assert!(state(&mut app).delete.is_some());
        click(&mut app, "确定")?;
        assert!(state(&mut app).mappings.is_empty());
        assert_eq!(
            std::fs::read_to_string(directory.path().join("port-mappings.json"))?.trim(),
            "[]"
        );
    }
    assert_eq!(std::fs::read(config)?, original);
    Ok(())
}

#[test]
fn saved_ftp_server_controls_start_transfer_stop_and_keep_legacy_fields() -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir()?;
    let shared = directory.path().join("share");
    std::fs::create_dir(&shared)?;
    let config = directory.path().join("connections.json");
    let mut connection = Connection::new("FTP_SERVER");
    connection.name = "兼容 FTP 服务".into();
    connection.set(
        "serverConfig",
        serde_json::json!({
            "type": "FTP",
            "port": 0,
            "bindAddress": "127.0.0.1",
            "rootDirectory": shared,
            "futureServerOption": {"retained": true},
            "accounts": [{"username": "fixture", "password": "fixture-only", "unknown": 42}],
        }),
    );
    let mut store = Store::default();
    store.connections.push(connection.clone());
    store.save(&config)?;

    let mut app = app(directory.path())?;
    app.open_connection(connection);
    click(&mut app, "启动服务")?;
    let started = Instant::now();
    let address = loop {
        app.logic(&app.context.clone());
        let tab = &app.tabs[0];
        anyhow::ensure!(tab.error.is_none(), "{:?}", tab.error);
        if let Panel::Server(view) = &tab.panel
            && let Some(server) = &view.running
        {
            break server.address;
        }
        anyhow::ensure!(started.elapsed() < Duration::from_secs(5), "FTP 服务未启动");
        std::thread::sleep(Duration::from_millis(10));
    };

    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut control = BufReader::new(socket);
    let mut reply = String::new();
    control.read_line(&mut reply)?;
    assert!(reply.starts_with("220 "));
    let mut send = |command: &str, code: &str| -> anyhow::Result<()> {
        writeln!(control.get_mut(), "{command}\r")?;
        reply.clear();
        control.read_line(&mut reply)?;
        anyhow::ensure!(reply.starts_with(code), "{command} returned {reply:?}");
        Ok(())
    };
    send("USER fixture", "331 ")?;
    send("PASS fixture-only", "230 ")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let [hi, lo] = listener.local_addr()?.port().to_be_bytes();
    send(&format!("PORT 127,0,0,1,{hi},{lo}"), "200 ")?;
    let (mut data, _) = listener.accept()?;
    send("STOR 目录/文件.txt", "150 ")?;
    data.write_all(b"through normal UI")?;
    data.shutdown(std::net::Shutdown::Write)?;
    reply.clear();
    control.read_line(&mut reply)?;
    assert!(reply.starts_with("226 "), "{reply:?}");
    assert_eq!(
        std::fs::read(shared.join("目录/文件.txt"))?,
        b"through normal UI"
    );

    click(&mut app, "停止服务")?;
    assert!(matches!(&app.tabs[0].panel, Panel::Server(view) if view.running.is_none()));
    reply.clear();
    let stopped = control.read_line(&mut reply);
    assert!(
        matches!(stopped, Ok(0))
            || stopped.is_err_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                )
            })
    );
    let persisted = Store::load(&config)?;
    let legacy = &persisted.connections[0].options["serverConfig"];
    assert_eq!(legacy["futureServerOption"]["retained"], true);
    assert_eq!(legacy["accounts"][0]["unknown"], 42);
    assert_eq!(legacy["accounts"][0]["password"], "fixture-only");
    assert_eq!(legacy["port"], 0);
    Ok(())
}

#[test]
fn saved_smb_server_starts_and_stops_from_the_original_controls() -> anyhow::Result<()> {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir()?;
    let share = directory.path().join("share");
    std::fs::create_dir(&share)?;
    let path = directory.path().join("connections.json");
    let mut connection = Connection::new("SMB_SERVER");
    connection.name = "兼容 SMB 服务".into();
    connection.set(
        "serverConfig",
        serde_json::json!({
            "type": "SMB", "port": 0, "bindAddress": "127.0.0.1",
            "rootDirectory": share, "unknownServerField": true,
            "accounts": [{"username": "fixture", "password": "fixture-only"}],
        }),
    );
    let mut store = Store::default();
    store.connections.push(connection.clone());
    store.save(&path)?;
    let mut app = app(directory.path())?;
    app.open_connection(connection);
    click(&mut app, "启动服务")?;
    let start = Instant::now();
    let address = loop {
        app.logic(&app.context.clone());
        let tab = &app.tabs[0];
        anyhow::ensure!(tab.error.is_none(), "{:?}", tab.error);
        if let Panel::Server(view) = &tab.panel
            && let Some(server) = &view.running
        {
            break server.address;
        }
        anyhow::ensure!(start.elapsed() < Duration::from_secs(5), "SMB 服务未启动");
        std::thread::sleep(Duration::from_millis(10));
    };
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    click(&mut app, "停止服务")?;
    assert!(matches!(&app.tabs[0].panel, Panel::Server(view) if view.running.is_none()));
    let mut byte = [0];
    let stopped = std::io::Read::read(&mut &stream, &mut byte);
    assert!(
        matches!(stopped, Ok(0))
            || stopped.is_err_and(|err| {
                matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                )
            })
    );
    let persisted = Store::load(&path)?;
    let legacy = &persisted.connections[0].options["serverConfig"];
    assert_eq!(legacy["unknownServerField"], true);
    assert_eq!(legacy["port"], 0);
    Ok(())
}
