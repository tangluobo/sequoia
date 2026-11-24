//! Native egui inputs and actual loopback service use cases. Never writes OS input/clipboard.
use super::*;
use anyhow::Context;
use std::time::{Duration, Instant};
fn frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    app.logic(&ctx);
    let time = ctx.input(|i| i.time) + 0.02;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            events,
            time: Some(time),
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}
fn position(app: &mut TomatoApp, label: &str, last: bool) -> anyhow::Result<egui::Pos2> {
    for _ in 0..4 {
        let output = frame(app, vec![]);
        let points = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(p) = if last { points.last() } else { points.first() } {
            return Ok(*p);
        }
    }
    anyhow::bail!("Missing actual control {label}")
}
fn pointer(app: &mut TomatoApp, pos: egui::Pos2, count: usize) {
    frame(app, vec![egui::Event::PointerMoved(pos)]);
    for _ in 0..count {
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
    }
}
fn click(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    wait(app, |app| app.tabs.iter().all(|tab| !tab.busy))?;
    eprintln!("Native control: {label}");
    let p = position(app, label, matches!(label, "主题" | "消息"))?;
    pointer(app, p, 1);
    Ok(())
}
fn context_menu(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    let point = position(app, label, false)?;
    frame(app, vec![egui::Event::PointerMoved(point)]);
    for pressed in [true, false] {
        frame(
            app,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            }],
        );
    }
    Ok(())
}
fn double(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    eprintln!("Native control: {label}");
    let p = position(app, label, false)?;
    // Separate double-click gestures so a later pair cannot become a triple click.
    for _ in 0..36 {
        frame(app, vec![]);
    }
    pointer(app, p, 2);
    eprintln!(
        "After double {label}: selected={:?}; hosts={:?}",
        app.selected_connection,
        app.remote_hosts
            .values()
            .map(|h| (
                &h.connection.name,
                h.ready,
                h.busy,
                h.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>()
    );
    Ok(())
}
fn wait(app: &mut TomatoApp, predicate: impl Fn(&TomatoApp) -> bool) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        frame(app, vec![]);
        if predicate(app) {
            return Ok(());
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(40),
            "Native workflow timeout; app={:?}; tabs={:?}",
            app.error,
            app.tabs
                .iter()
                .map(|t| (&t.title, &t.error, t.busy))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn new_app(root: &std::path::Path) -> anyhow::Result<TomatoApp> {
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    Ok(TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(root.join("connections.json")),
    ))
}
fn cloud(app: &TomatoApp) -> &crate::tabs::cloud::State {
    let t = app
        .tabs
        .iter()
        .find(|t| Some(&t.id) == app.active.as_ref())
        .unwrap();
    let Panel::Cloud { view, .. } = &t.panel else {
        panic!("cloud tab")
    };
    view
}
fn cloud_mut(app: &mut TomatoApp) -> &mut crate::tabs::cloud::State {
    let t = app
        .tabs
        .iter_mut()
        .find(|t| Some(&t.id) == app.active.as_ref())
        .unwrap();
    let Panel::Cloud { view, .. } = &mut t.panel else {
        panic!("cloud tab")
    };
    view
}
fn mq(app: &TomatoApp) -> &crate::tabs::mq::State {
    let t = app
        .tabs
        .iter()
        .find(|t| Some(&t.id) == app.active.as_ref())
        .unwrap();
    let Panel::MessageQueue { view, .. } = &t.panel else {
        panic!("MQ tab")
    };
    view
}
fn mq_mut(app: &mut TomatoApp) -> &mut crate::tabs::mq::State {
    let t = app
        .tabs
        .iter_mut()
        .find(|t| Some(&t.id) == app.active.as_ref())
        .unwrap();
    let Panel::MessageQueue { view, .. } = &mut t.panel else {
        panic!("MQ tab")
    };
    view
}
#[test]
#[ignore = "requires owned cloud/ACME/DNS/S3 loopback fixture"]
fn actual_cloud_tree_dns_dialog_ddns_certificate_and_delete() -> anyhow::Result<()> {
    let fixture = PathBuf::from(std::env::var("TOMATO_CLOUD_FIXTURE_DIR")?);
    anyhow::ensure!(
        fixture
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with("tomato-cloud-fixture-")),
        "Requires owned fixture"
    );
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("ready.json"))?)?;
    let endpoint = ready["endpoint"].as_str().unwrap();
    anyhow::ensure!(
        endpoint.starts_with("http://127.0.0.1:"),
        "Loopback fixture required"
    );
    for kind in ["TENCENT_CLOUD", "ALIYUN"] {
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("connections.json");
        Store::default().save(&config)?;
        let before = std::fs::read(&config)?;
        let mut app = new_app(directory.path())?;
        let mut c = Connection::new(kind);
        c.name = if kind == "ALIYUN" {
            "隔离阿里云"
        } else {
            "隔离腾讯云"
        }
        .into();
        c.set("certificateEmail", "legacy@owned.test");
        c.set("username", "fixture-id");
        c.set("password", "fixture-secret");
        c.set("cloudApiEndpoint", format!("{endpoint}/cloud"));
        c.set("acmeDirectory", format!("{endpoint}/acme/directory"));
        c.set("certificateDnsServer", ready["dns"].as_str().unwrap());
        let mut storage = Connection::new("S3");
        storage.name = "隔离证书存储".into();
        storage.set("endpoint", endpoint);
        storage.set("username", "fixture-storage");
        storage.set("savePassword", false);
        storage.set("region", "us-east-1");
        storage.set("pathStyleAccess", true);
        if kind == "TENCENT_CLOUD" {
            storage.set("bucket", "cert-bucket");
        }
        app.store.connections.extend([c.clone(), storage]);
        double(&mut app, &c.name)?;
        wait(&mut app, |app| {
            app.remote_hosts.get(&c.id).is_some_and(|h| h.ready)
        })?;
        double(
            &mut app,
            if kind == "ALIYUN" {
                "域名管理"
            } else {
                "DNSPod 域名解析"
            },
        )?;
        wait(&mut app, |app| {
            app.remote_hosts[&c.id]
                .entries
                .iter()
                .any(|e| e.name == "owned.test")
        })?;
        double(&mut app, "owned.test")?;
        wait(&mut app, |app| {
            !app.tabs.is_empty() && cloud(app).loaded && !app.tabs.last().unwrap().busy
        })?;
        click(&mut app, "添加")?;
        let record = cloud_mut(&mut app).record.as_mut().unwrap();
        record.rr = "ui-home".into();
        record.value = "192.0.2.61".into();
        click(&mut app, "确定")?;
        wait(&mut app, |app| {
            cloud(app).rows.iter().any(|v| v["value"] == "192.0.2.61")
        })?;
        double(&mut app, "ui-home")?;
        cloud_mut(&mut app).record.as_mut().unwrap().value = "192.0.2.62".into();
        click(&mut app, "确定")?;
        wait(&mut app, |app| {
            cloud(app).rows.iter().any(|v| v["value"] == "192.0.2.62")
        })?;
        for enabled in [true, false] {
            let row = position(&mut app, "ui-home", false)?;
            let output = frame(&mut app, vec![]);
            let switch = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Circle(c)
                        if (c.radius - 7.).abs() < 0.1 && (c.center.y - row.y).abs() < 3. =>
                    {
                        Some(c.center)
                    }
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("Actual DDNS switch missing"))?;
            pointer(&mut app, switch, 1);
            wait(&mut app, |app| {
                cloud(app)
                    .rows
                    .first()
                    .is_some_and(|v| v["ddns"].as_bool() == Some(enabled))
            })?;
            assert_eq!(
                tomato_services::cloud::ddns::load(&directory.path().join("ddns.json"))?.len(),
                usize::from(enabled)
            );
        }
        if kind == "ALIYUN" {
            context_menu(&mut app, "owned.test")?;
            click(&mut app, "申请 Let's Encrypt 证书")?;
        } else {
            context_menu(&mut app, "owned.test")?;
            let menu = position(&mut app, "申请证书", true)?;
            pointer(&mut app, menu, 1);
            wait(&mut app, |app| cloud(app).certificate.is_some())?;
            let request = &cloud(&app).certificate.as_ref().unwrap().request;
            assert_eq!(
                (&*request.domain, &*request.dns_zone),
                ("owned.test", "owned.test")
            );
            assert!(app.credential.is_none() && !app.tabs.last().unwrap().busy);
            assert_eq!(request.bucket, "cert-bucket");
            click(&mut app, "取消")?;
            wait(&mut app, |app| cloud(app).certificate.is_none())?;

            context_menu(&mut app, "DNSPod 域名解析")?;
            click(&mut app, "打开域名列表")?;
            wait(&mut app, |app| {
                matches!(cloud(app).page, crate::tabs::cloud::Page::Domains)
                    && cloud(app).loaded
                    && cloud(app)
                        .rows
                        .iter()
                        .any(|row| row["domainName"] == "owned.test")
            })?;
            click(&mut app, "申请证书")?;
        }
        if kind == "ALIYUN" {
            assert!(
                app.credential.is_none(),
                "an optional bucket lookup must not prompt automatically"
            );
            let refresh = position(&mut app, "刷新", true)?;
            pointer(&mut app, refresh, 1);
            supply_temporary_storage_key(&mut app)?;
            wait(&mut app, |app| {
                cloud(app)
                    .certificate
                    .as_ref()
                    .is_some_and(|c| c.bucket_error.is_some() && c.bucket_task.is_none())
            })?;
            cloud_mut(&mut app)
                .certificate
                .as_mut()
                .unwrap()
                .request
                .bucket = "cert-bucket".into();
        } else {
            let dialog = cloud(&app).certificate.as_ref().unwrap();
            assert_eq!(dialog.request.bucket, "cert-bucket");
            assert!(dialog.buckets.is_empty() && dialog.bucket_task.is_none());
            assert!(app.credential.is_none());
        }
        assert!(
            app.store
                .connections
                .iter()
                .find(|c| c.kind() == "S3")
                .unwrap()
                .text("password")
                .is_empty()
        );
        let request = &mut cloud_mut(&mut app).certificate.as_mut().unwrap().request;
        assert_eq!(
            (&*request.domain, &*request.dns_zone),
            ("owned.test", "owned.test")
        );
        assert_eq!(request.email, "legacy@owned.test");
        request.email = "owner@owned.test".into();
        request.wildcard = true;
        request.zip_package = false;
        request.keystore_password = "fixture-password".into();
        if kind == "TENCENT_CLOUD" {
            let refresh = position(&mut app, "刷新", true)?;
            pointer(&mut app, refresh, 1);
            supply_temporary_storage_key(&mut app)?;
            assert!(
                cloud(&app)
                    .certificate
                    .as_ref()
                    .unwrap()
                    .bucket_task
                    .is_some()
            );
            assert!(
                !app.tabs.last().unwrap().busy,
                "list request must not lock the form"
            );
            click(&mut app, "申请")?;
            assert!(app.tabs.last().unwrap().busy);
        } else {
            click(&mut app, "申请")?;
        }
        anyhow::ensure!(app.credential.is_none(), "申请应复用此次输入的 S3 密钥");
        wait(&mut app, |app| {
            cloud(app).certificate.is_none() && cloud(app).status.contains("证书已保存")
        })?;
        if kind == "TENCENT_CLOUD" {
            double(&mut app, "owned.test")?;
        }
        click(&mut app, "删除")?;
        click(&mut app, "确定")?;
        wait(&mut app, |app| {
            cloud(app).rows.is_empty() && !app.tabs.last().unwrap().busy
        })?;
        assert_eq!(std::fs::read(&config)?, before);
        std::fs::write(
            fixture.join("ui-cloud-passed"),
            "native-controls-and-protocol-flow",
        )?;
    }
    Ok(())
}

fn supply_temporary_storage_key(app: &mut TomatoApp) -> anyhow::Result<()> {
    wait(app, |app| app.credential.is_some())?;
    frame(
        app,
        vec![egui::Event::Text("fixture-storage-secret".into())],
    );
    click(app, "确定")
}
#[test]
#[ignore = "requires owned Kafka and RocketMQ brokers"]
fn actual_mq_tree_create_send_query_and_delete_controls() -> anyhow::Result<()> {
    for kind in ["KAFKA", "ROCKETMQ"] {
        let endpoint = std::env::var(if kind == "KAFKA" {
            "TOMATO_KAFKA_FIXTURE"
        } else {
            "TOMATO_ROCKET_FIXTURE"
        })?;
        let endpoint: std::net::SocketAddr = endpoint.parse()?;
        anyhow::ensure!(endpoint.ip().is_loopback(), "Loopback required");
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("connections.json");
        Store::default().save(&config)?;
        let before = std::fs::read(&config)?;
        let mut app = new_app(directory.path())?;
        let mut c = Connection::new(kind);
        c.name = format!("隔离{kind}");
        c.set("host", endpoint.ip().to_string());
        c.set("port", endpoint.port());
        app.store.connections.push(c.clone());
        double(&mut app, &c.name)?;
        wait(&mut app, |app| {
            app.remote_hosts.get(&c.id).is_some_and(|h| h.ready)
        })?;
        double(&mut app, "主题")?;
        wait(&mut app, |app| {
            !app.tabs.is_empty() && mq(app).loaded && !app.tabs.last().unwrap().busy
        })?;
        click(&mut app, "创建主题")?;
        let name = format!("tomato_ui_{}", Connection::new("TOOL").id.replace('-', ""));
        let view = mq_mut(&mut app);
        view.new_topic = name.clone();
        view.partitions = 2;
        view.replicas = 1;
        click(&mut app, "确定")?;
        wait(&mut app, |app| {
            mq(app).rows.iter().any(|v| v["topic"] == name)
                && app.remote_hosts[&c.id]
                    .entries
                    .iter()
                    .any(|e| e.name == name)
        })?;
        double(&mut app, &name)?;
        wait(&mut app, |app| {
            !mq(app).topic.is_empty() && mq(app).loaded && !app.tabs.last().unwrap().busy
        })?;
        click(&mut app, "消息")?;
        click(&mut app, "发送消息")?;
        let view = mq_mut(&mut app);
        view.send_key = "native-key".into();
        view.tags = "native-tag".into();
        view.send_partition = "0".into();
        view.headers = "native=value".into();
        view.send_body = "{\"消息\":\"原生按钮实际发送\"}".into();
        click(&mut app, "发送")?;
        wait(&mut app, |app| {
            mq(app).status.contains("发送成功") || mq(app).status == "发送成功"
        })?;
        if kind == "KAFKA" {
            click(&mut app, "按Offset查询")?;
            let view = mq_mut(&mut app);
            view.partition = "0".into();
            view.offset = "0".into();
        } else {
            let result: serde_json::Value = serde_json::from_str(&mq(&app).detail)?;
            let id = result["msgId"].as_str().unwrap().to_owned();
            click(&mut app, "按Message ID查询")?;
            mq_mut(&mut app).id = id;
        }
        click(&mut app, "查询")?;
        wait(&mut app, |app| {
            mq(app)
                .rows
                .iter()
                .any(|v| v["body"] == "{\"消息\":\"原生按钮实际发送\"}")
        })?;
        click(&mut app, "按时间查询")?;
        click(&mut app, "近1天")?;
        click(&mut app, "查询")?;
        wait(&mut app, |app| {
            mq(app).rows.len() == 1 && !app.tabs.last().unwrap().busy
        })?;
        // Return through the actual topic tab and delete from its list.
        click(&mut app, "主题")?;
        click(&mut app, "返回列表")?;
        wait(&mut app, |app| {
            mq(app).topic.is_empty()
                && mq(app).loaded
                && app.tabs.iter().all(|tab| !tab.busy)
                && mq(app).rows.iter().any(|v| v["topic"] == name)
        })?;
        frame(&mut app, vec![]);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let index = mq(&app)
            .rows
            .iter()
            .position(|v| v["topic"] == name)
            .context("Created topic missing from actual list")?;
        let mut target = None;
        for _ in 0..30 {
            let visible = app
                .context
                .data(|data| {
                    data.get_temp::<Vec<(usize, egui::Rect)>>(egui::Id::new(
                        "tomato-remote-visible-rows",
                    ))
                })
                .unwrap_or_default();
            if let Some((_, rect)) = visible.iter().find(|(i, r)| *i == index && r.is_positive()) {
                target = Some(*rect);
                break;
            }
            let (visible_index, rect) = visible
                .iter()
                .find(|(_, r)| r.is_positive())
                .context("Actual native topic table has no visible row controls")?;
            frame(&mut app, vec![egui::Event::PointerMoved(rect.center())]);
            frame(
                &mut app,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0., if index > *visible_index { -400. } else { 400. }),
                    modifiers: Default::default(),
                }],
            );
            frame(&mut app, vec![]);
        }
        let rect = target.context("Created topic did not become visible after native scrolling")?;
        let row = rect.center();
        eprintln!("Actual topic row {index}: {rect:?}");
        pointer(&mut app, row, 1);
        eprintln!(
            "Before topic delete: selected={:?}; topic={:?}; page={:?}",
            mq(&app).selected,
            mq(&app).topic,
            mq(&app).page
        );
        anyhow::ensure!(
            mq(&app).selected == Some(index),
            "Actual topic row click selected the wrong row"
        );
        click(&mut app, "删除主题")?;
        eprintln!(
            "After topic delete click: selected={:?}; confirm={:?}; topic={:?}",
            mq(&app).selected,
            mq(&app).confirm,
            mq(&app).topic
        );
        anyhow::ensure!(
            matches!(&mq(&app).confirm,Some(tomato_services::mq::Action::DeleteTopic{topic}) if topic==&name),
            "Delete confirmation targets a different topic"
        );
        click(&mut app, "确定")?;
        wait(&mut app, |app| {
            !mq(app).rows.iter().any(|v| v["topic"] == name) && !app.tabs.last().unwrap().busy
        })?;
        assert_eq!(std::fs::read(config)?, before);
    }
    Ok(())
}

#[test]
fn initial_cloud_and_mq_errors_wait_for_explicit_refresh() -> anyhow::Result<()> {
    for kind in ["TENCENT_CLOUD", "KAFKA"] {
        let directory = tempfile::tempdir()?;
        Store::default().save(&directory.path().join("connections.json"))?;
        let mut app = new_app(directory.path())?;
        let mut c = Connection::new(kind);
        c.name = format!("隔离失败场景 {kind}");
        c.set("host", "");
        c.set("port", 0);
        c.set("cloudApiEndpoint", "http://127.0.0.1:0/cloud");
        let mut tab = crate::tabs::create_tab(c.clone(), app.runtime.as_ref().map(|r| r.handle()))?;
        if let Panel::Cloud { view, .. } = &mut tab.panel {
            view.page = crate::tabs::cloud::Page::Records("owned.test".into());
            view.ddns_path = directory.path().join("ddns.json");
        }
        let id = tab.id.clone();
        app.store.connections.push(c);
        app.active = Some(id.clone());
        app.tabs.push(tab);
        wait(&mut app, |app| {
            app.tabs
                .iter()
                .any(|t| t.id == id && t.error.is_some() && !t.busy)
        })?;
        for _ in 0..30 {
            frame(&mut app, vec![]);
            assert!(app.pending.is_empty());
            assert!(!app.tabs.last().unwrap().busy);
        }
        click(&mut app, "刷新")?;
        assert!(app.tabs.last().unwrap().busy);
        wait(&mut app, |app| {
            app.tabs
                .last()
                .is_some_and(|t| t.error.is_some() && !t.busy)
        })?;
    }
    Ok(())
}
