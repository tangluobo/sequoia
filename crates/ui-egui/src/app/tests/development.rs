use super::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tomato_core::development::{Config, Project, Runtime};

/// Drives the real settings viewports and background repository, never OS input.
struct SettingsUi(super::native_bridge::Bridge);
impl SettingsUi {
    fn frame(&self, app: &mut TomatoApp, events: Vec<egui::Event>) {
        let ctx = app.context.clone();
        self.0.root_frame(&ctx, vec![], events, |ui| app.ui(ui));
    }
    fn point(&self, app: &mut TomatoApp, pos: egui::Pos2) {
        for pressed in [true, false] {
            self.frame(
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
        self.frame(app, vec![]);
    }
    fn label(&self, app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
        self.until(app, |s| s.0.text(label).is_ok())?;
        self.point(app, self.0.text(label)?);
        Ok(())
    }
    fn until(&self, app: &mut TomatoApp, condition: impl Fn(&Self) -> bool) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.frame(app, vec![]);
            if condition(self) {
                return Ok(());
            }
            anyhow::ensure!(Instant::now() < deadline, "settings viewport timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn fill(&self, app: &mut TomatoApp, hint: &str, value: &str) -> anyhow::Result<()> {
        self.label(app, hint)?;
        self.frame(app, vec![egui::Event::Text(value.into())]);
        Ok(())
    }
}
fn open_settings(app: &mut TomatoApp) -> anyhow::Result<()> {
    let output = frame(app, vec![], Default::default());
    let pos = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "fixture project" => {
                Some(t.pos + t.galley.size() / 2.)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing fixture project row"))?;
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            Default::default(),
        );
    }
    click(app, "配置")
}

#[test]
fn real_development_sdk_dialog_validation_cancel_save_and_selection() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let sdk = root.join("owned SDK");
    std::fs::create_dir(&sdk)?;
    let mut app = setup(root, None)?;
    let original = std::fs::read(root.join("development.json"))?;
    let ui = SettingsUi(super::native_bridge::Bridge::new(&app.context));
    for save in [false, true] {
        open_settings(&mut app)?;
        ui.label(&mut app, "SDK 管理")?;
        ui.label(&mut app, "添加 SDK")?;
        ui.label(&mut app, "添加")?;
        ui.until(&mut app, |s| {
            s.0.text("请填写名称并选择有效的 JDK 路径").is_ok()
        })?;
        ui.label(&mut app, "确定")?;
        ui.fill(&mut app, "SDK 名称", "fixture SDK")?;
        ui.fill(&mut app, "SDK 安装路径", &sdk.to_string_lossy())?;
        ui.label(&mut app, "添加")?;
        ui.until(&mut app, |s| s.0.text("fixture SDK  未知版本").is_ok())?;
        if save {
            ui.label(&mut app, "项目配置")?;
            ui.label(&mut app, "未指定 JDK")?;
            ui.label(&mut app, "fixture SDK (未知版本)")?;
            ui.label(&mut app, "确定")?;
        } else {
            ui.label(&mut app, "取消")?;
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while app.development.as_ref().unwrap().probe().settings_open {
            ui.frame(&mut app, vec![]);
            anyhow::ensure!(
                Instant::now() < deadline,
                "settings did not close after save/cancel"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if !save {
            assert_eq!(std::fs::read(root.join("development.json"))?, original);
            assert!(!root.join("development.json.before-rust.bak").exists());
        }
    }
    let (saved, _) =
        tomato_services::development::repository::load(&root.join("development.json"))?;
    assert_eq!(saved.runtimes.len(), 1);
    assert_eq!(saved.runtimes[0].name, "fixture SDK");
    assert_eq!(saved.projects[0].jdk_id, saved.runtimes[0].id);
    assert_eq!(saved.extra["futureField"]["keep"], true);
    assert_eq!(
        std::fs::read(root.join("development.json.before-rust.bak"))?,
        original
    );
    assert!(sdk.is_dir());
    assert!(!root.join("connections.json").exists());
    app.shutdown_file_transfers();
    Ok(())
}

#[test]
fn real_development_add_directory_validates_and_confirms_duplicate_names() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let project = root.join("second owned project");
    std::fs::create_dir(&project)?;
    std::fs::write(project.join("pom.xml"), "<project/>")?;
    let mut app = setup(root, None)?;
    let original = std::fs::read(root.join("development.json"))?;
    let ui = SettingsUi(super::native_bridge::Bridge::new(&app.context));
    let pos = egui::pos2(150., 600.);
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            Default::default(),
        );
    }
    click(&mut app, "添加目录")?;
    ui.label(&mut app, "确定")?;
    ui.until(&mut app, |s| s.0.text("目录名称和路径不能为空").is_ok())?;
    ui.label(&mut app, "确定")?;
    ui.fill(&mut app, "如：backend", "FIXTURE PROJECT")?;
    ui.fill(&mut app, "选择一个本地目录路径", &project.to_string_lossy())?;
    ui.label(&mut app, "确定")?;
    ui.until(&mut app, |s| {
        s.0.text("目录名称已存在，仍要添加吗？").is_ok()
    })?;
    ui.label(&mut app, "否")?;
    assert_eq!(std::fs::read(root.join("development.json"))?, original);
    assert!(!root.join("development.json.before-rust.bak").exists());
    ui.label(&mut app, "确定")?;
    ui.until(&mut app, |s| {
        s.0.text("目录名称已存在，仍要添加吗？").is_ok()
    })?;
    ui.label(&mut app, "是")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.development.as_ref().unwrap().probe().add_open {
        ui.frame(&mut app, vec![]);
        anyhow::ensure!(Instant::now() < deadline, "directory was not persisted");
        std::thread::sleep(Duration::from_millis(10));
    }
    let (saved, _) =
        tomato_services::development::repository::load(&root.join("development.json"))?;
    assert_eq!(saved.projects.len(), 2);
    assert_eq!(saved.projects[1].name, "FIXTURE PROJECT");
    assert_eq!(saved.projects[1].project_type, "MAVEN");
    assert_eq!(saved.extra["futureField"]["keep"], true);
    assert_eq!(
        std::fs::read(root.join("development.json.before-rust.bak"))?,
        original
    );
    assert!(!root.join("connections.json").exists());
    app.shutdown_file_transfers();
    Ok(())
}
fn frame(
    app: &mut TomatoApp,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let ctx = app.context.clone();
    for event in &mut events {
        if let egui::Event::Key { modifiers, .. } | egui::Event::PointerButton { modifiers, .. } =
            event
            && modifiers.ctrl
        {
            modifiers.command = true;
        }
    }
    let mut modifiers = modifiers;
    if modifiers.ctrl {
        modifiers.command = true;
    }
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 750.),
            )),
            events,
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}
fn click_at(app: &mut TomatoApp, pos: egui::Pos2) {
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
            Default::default(),
        );
    }
}
fn click(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    let output = frame(app, vec![], Default::default());
    let pos = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing development control {label}"))?;
    click_at(app, pos);
    Ok(())
}
fn wait(app: &mut TomatoApp, condition: impl Fn(&TomatoApp) -> bool) -> anyhow::Result<()> {
    wait_for(app, 15, condition)
}
fn wait_for(
    app: &mut TomatoApp,
    timeout: u64,
    condition: impl Fn(&TomatoApp) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    while !condition(app) && Instant::now() < deadline {
        frame(app, vec![], Default::default());
        std::thread::sleep(Duration::from_millis(10));
    }
    anyhow::ensure!(
        condition(app),
        "development command chain timed out: {}",
        app.development
            .as_ref()
            .and_then(|s| s.probe().error)
            .or_else(|| app.development.as_ref().and_then(|s| s.probe().log))
            .unwrap_or(&app.status)
    );
    Ok(())
}
fn setup(root: &Path, node: Option<PathBuf>) -> anyhow::Result<TomatoApp> {
    let project_path = root.join("project");
    std::fs::create_dir(&project_path)?;
    let mut git = std::process::Command::new("git");
    git.args(["init", "--template=", "-q"])
        .current_dir(&project_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        git.creation_flags(0x08000000);
    }
    anyhow::ensure!(git.status()?.success(), "owned fixture git init failed");
    std::fs::write(project_path.join("app.txt"), "alpha\n")?;
    std::fs::write(project_path.join("other.txt"), "other\n")?;
    let mut project = Project {
        name: "fixture project".into(),
        path: project_path.to_string_lossy().into_owned(),
        project_type: "GENERAL".into(),
        ..Default::default()
    };
    let mut config = Config::default();
    if let Some(node) = node {
        project.project_type = "NODE".into();
        project.node_id = "node".into();
        project.run_type = "NPM".into();
        project.run_target = "dev".into();
        config.runtimes.push(Runtime {
            id: "node".into(),
            kind: "NODE".into(),
            path: node.to_string_lossy().into_owned(),
            ..Default::default()
        });
        std::fs::write(
            project_path.join("package.json"),
            r#"{"scripts":{"dev":"node app.js"}}"#,
        )?;
        std::fs::write(
            project_path.join("app.js"),
            "console.log('READY UI');console.log('[ERROR] link http://127.0.0.1:1234/hello?q=1).');setInterval(()=>console.log('tick'),100);",
        )?;
    }
    config
        .extra
        .insert("futureField".into(), serde_json::json!({"keep":true}));
    config.projects.push(project);
    std::fs::write(root.join("development.json"), serde_json::to_vec(&config)?)?;
    configured(root, config)
}
fn configured(root: &Path, config: Config) -> anyhow::Result<TomatoApp> {
    std::fs::write(root.join("development.json"), serde_json::to_vec(&config)?)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(root.join("connections.json")),
    );
    app.section = 3;
    frame(&mut app, vec![], Default::default());
    wait(&mut app, |app| {
        app.development.as_ref().is_some_and(|s| s.probe().loaded)
    })?;
    Ok(app)
}
#[test]
fn real_development_navigation_edit_save_unsaved_cancel_and_config_guard() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut app = setup(dir.path(), None)?;
    click(&mut app, "fixture project")?;
    wait(&mut app, |app| {
        app.development
            .as_ref()
            .is_some_and(|s| s.probe().ready && s.probe().files_loaded)
    })?;
    click(&mut app, "app.txt")?;
    wait(&mut app, |app| {
        app.development
            .as_ref()
            .is_some_and(|s| s.probe().document_path.is_some())
    })?;
    click(&mut app, "alpha\n")?;
    frame(
        &mut app,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            },
            egui::Event::Text("edited 中文\n".into()),
        ],
        egui::Modifiers::CTRL,
    );
    assert!(app.development.as_ref().unwrap().dirty());
    frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
        egui::Modifiers::CTRL,
    );
    wait(&mut app, |app| !app.development.as_ref().unwrap().dirty())?;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("project/app.txt"))?,
        "edited 中文\n"
    );
    click(&mut app, "edited 中文\n")?;
    frame(
        &mut app,
        vec![egui::Event::Text("draft".into())],
        Default::default(),
    );
    assert!(app.development.as_ref().unwrap().dirty());
    click(&mut app, "other.txt")?;
    assert!(app.development.as_ref().unwrap().probe().modal);
    click(&mut app, "取消")?;
    assert!(app.development.as_ref().unwrap().dirty());
    assert!(
        app.development
            .as_ref()
            .unwrap()
            .probe()
            .document_path
            .unwrap()
            .ends_with("app.txt")
    );
    click(&mut app, "other.txt")?;
    click(&mut app, "不保存")?;
    wait(&mut app, |app| {
        app.development
            .as_ref()
            .unwrap()
            .probe()
            .document_path
            .is_some_and(|path| path.ends_with("other.txt"))
    })?;
    assert!(!dir.path().join("connections.json").exists());
    let (config, _) =
        tomato_services::development::repository::load(&dir.path().join("development.json"))?;
    assert_eq!(config.extra["futureField"]["keep"], true);
    app.shutdown_file_transfers();
    Ok(())
}
#[test]
#[ignore = "requires TOMATO_DEV_NODE pointing at real Node; actual development controls and owned temporary npm project"]
fn actual_development_start_and_stop_buttons_run_the_configured_npm_script() -> anyhow::Result<()> {
    let node = PathBuf::from(
        std::env::var_os("TOMATO_DEV_NODE")
            .ok_or_else(|| anyhow::anyhow!("missing TOMATO_DEV_NODE"))?,
    );
    let dir = tempfile::tempdir()?;
    let mut app = setup(dir.path(), Some(node))?;
    click(&mut app, "fixture project")?;
    wait(&mut app, |app| {
        app.development.as_ref().unwrap().probe().ready
    })?;
    frame(&mut app, vec![], Default::default());
    let button = app
        .context
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("development-control", "启动"))))
        .ok_or_else(|| anyhow::anyhow!("missing real start button"))?;
    let clip = app
        .context
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("development-start-clip")))
        .unwrap();
    assert!(
        clip.contains(button.center()),
        "Start button {button:?} clipped by {clip:?}"
    );
    click_at(&mut app, button.center());
    assert!(
        app.context
            .data(|d| d.get_temp::<bool>(egui::Id::new("development-start-clicked")))
            .unwrap_or(false),
        "Start button did not receive pointer click"
    );
    wait(&mut app, |app| {
        app.development
            .as_ref()
            .unwrap()
            .probe()
            .log
            .is_some_and(|text| {
                text.contains("READY UI") && text.contains("http://127.0.0.1:1234/hello?q=1")
            })
    })?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let link = loop {
        let output = frame(&mut app, vec![], Default::default());
        if let Some(link) = output.shapes.iter().find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape
                && let Some(at) = t.galley.job.text.find("http://127.0.0.1:1234/hello?q=1")
            {
                let index = t.galley.job.text[..at].chars().count() + 9;
                let cursor = t.galley.pos_from_cursor(egui::text::CCursor::new(index));
                Some(t.pos + cursor.center().to_vec2())
            } else {
                None
            }
        }) {
            break link;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "real process log did not render its link"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut opened = false;
    for pressed in [true, false] {
        let output = frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(link),
                egui::Event::PointerButton {
                    pos: link,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            Default::default(),
        );
        opened|=output.platform_output.commands.iter().any(|cmd|matches!(cmd,egui::OutputCommand::OpenUrl(url) if url.url=="http://127.0.0.1:1234/hello?q=1"));
    }
    assert!(
        opened,
        "click must emit the original punctuation-trimmed browser action without running a browser in this test"
    );
    let stop = app
        .context
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("development-control", "停止"))))
        .ok_or_else(|| anyhow::anyhow!("missing original stop square"))?;
    click_at(&mut app, stop.center());
    wait(&mut app, |app| {
        !app.development.as_ref().unwrap().probe().running
    })?;
    let (config, _) =
        tomato_services::development::repository::load(&dir.path().join("development.json"))?;
    assert_eq!(config.extra["futureField"]["keep"], true);
    assert!(dir.path().join("development.json.before-rust.bak").exists());
    app.shutdown_file_transfers();
    Ok(())
}

#[test]
#[ignore = "requires isolated copied Maven cache and real TOMATO_DEV_JDK/TOMATO_DEV_MAVEN; genuine file tree context menu to offline compile, Java launch and log"]
fn actual_java_file_context_menu_compiles_runs_and_preserves_legacy_config() -> anyhow::Result<()> {
    use tomato_core::development::BuildTool;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let project_path = root.join("project");
    std::fs::create_dir_all(project_path.join("src/main/java/demo"))?;
    std::fs::create_dir(project_path.join(".mvn"))?;
    let mut git = std::process::Command::new("git");
    git.args(["init", "--template=", "-q"])
        .current_dir(&project_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        git.creation_flags(0x08000000);
    }
    anyhow::ensure!(git.status()?.success(), "owned Git initialization failed");
    let repository = PathBuf::from(
        std::env::var_os("TOMATO_DEV_MAVEN_REPO")
            .ok_or_else(|| anyhow::anyhow!("missing owned Maven cache"))?,
    );
    anyhow::ensure!(
        repository.file_name().is_some_and(|n| n
            .to_string_lossy()
            .starts_with("development-maven-fixture-")),
        "must use own Maven cache"
    );
    let settings = project_path.join("settings.xml");
    std::fs::write(
        &settings,
        format!(
            "<settings><localRepository>{}</localRepository><offline>true</offline></settings>",
            repository
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
        ),
    )?;
    std::fs::write(
        project_path.join(".mvn/maven.config"),
        format!("-o\n-gs\n{}\n", settings.display()),
    )?;
    std::fs::write(
        project_path.join("pom.xml"),
        r#"<project><modelVersion>4.0.0</modelVersion><groupId>invalid.fixture</groupId><artifactId>tomato-owned</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties><build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-resources-plugin</artifactId><version>3.3.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-dependency-plugin</artifactId><version>2.8</version></plugin></plugins></build></project>"#,
    )?;
    std::fs::write(
        project_path.join("src/main/java/demo/App.java"),
        "package demo; public class App { public static void main(String[] args) { System.out.println(\"JAVA READY 中文 \"+System.getProperty(\"spring.profiles.active\")); } }",
    )?;
    let project = Project {
        name: "fixture Java".into(),
        path: project_path.to_string_lossy().into_owned(),
        project_type: "MAVEN".into(),
        jdk_id: "jdk".into(),
        maven_id: "maven".into(),
        active_profiles: vec!["fixture".into()],
        ..Default::default()
    };
    let mut config = Config {
        projects: vec![project],
        runtimes: vec![Runtime {
            id: "jdk".into(),
            kind: "JDK".into(),
            path: std::env::var("TOMATO_DEV_JDK")?,
            ..Default::default()
        }],
        build_tools: vec![BuildTool {
            id: "maven".into(),
            kind: "MAVEN".into(),
            home_path: std::env::var("TOMATO_DEV_MAVEN")?,
            settings_path: settings.to_string_lossy().into_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };
    config
        .extra
        .insert("futureField".into(), serde_json::json!({"keep":true}));
    let mut app = configured(root, config)?;
    click(&mut app, "fixture Java")?;
    wait(&mut app, |a| a.development.as_ref().unwrap().probe().ready)?;
    for label in ["src", "main", "java", "demo"] {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let output = frame(&mut app, vec![], Default::default());
            let arrow = output.shapes.iter().find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape
                    && t.galley.job.text == label
                {
                    Some(egui::pos2(t.pos.x - 30., t.pos.y + t.galley.size().y / 2.))
                } else {
                    None
                }
            });
            if let Some(pos) = arrow {
                click_at(&mut app, pos);
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "file tree did not expose {label}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let pos = loop {
        let output = frame(&mut app, vec![], Default::default());
        if let Some(pos) = output.shapes.iter().find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape
                && t.galley.job.text == "App.java"
            {
                Some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        }) {
            break pos;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "Java source file is absent from real tree"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            Default::default(),
        );
    }
    click(&mut app, "运行 App.main()")?;
    wait_for(&mut app, 90, |a| {
        a.development
            .as_ref()
            .unwrap()
            .probe()
            .log
            .is_some_and(|s| s.contains("JAVA READY 中文 fixture"))
    })?;
    wait(&mut app, |a| {
        !a.development.as_ref().unwrap().probe().running
    })?;
    assert!(project_path.join("target/classes/demo/App.class").is_file());
    let (saved, _) =
        tomato_services::development::repository::load(&root.join("development.json"))?;
    assert_eq!(saved.extra["futureField"]["keep"], true);
    assert_eq!(
        saved.projects[0].main_source_path.replace('\\', "/"),
        "src/main/java/demo/App.java"
    );
    assert!(root.join("development.json.before-rust.bak").is_file());
    assert!(!root.join("connections.json").exists());
    app.shutdown_file_transfers();
    Ok(())
}

#[test]
fn real_development_build_tool_type_change_save_and_cancelled_delete() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let project = root.join("project");
    let tools = root.join("owned tools");
    std::fs::create_dir(&project)?;
    std::fs::create_dir(&tools)?;
    let mut config = Config::default();
    config.projects.push(Project {
        name: "fixture project".into(),
        path: project.to_string_lossy().into_owned(),
        project_type: "MAVEN".into(),
        maven_id: "legacy-tool".into(),
        ..Default::default()
    });
    let mut tool = tomato_core::development::BuildTool {
        id: "legacy-tool".into(),
        kind: "MAVEN".into(),
        name: "legacy Maven".into(),
        version: "3.8.0".into(),
        home_path: tools.to_string_lossy().into_owned(),
        settings_path: "old-settings.xml".into(),
        ..Default::default()
    };
    tool.extra
        .insert("futureToolField".into(), serde_json::json!({"keep":true}));
    config.build_tools.push(tool);
    let mut app = configured(root, config)?;
    let original = std::fs::read(root.join("development.json"))?;
    let ui = SettingsUi(super::native_bridge::Bridge::new(&app.context));
    open_settings(&mut app)?;
    ui.label(&mut app, "构建管理")?;
    ui.label(&mut app, "legacy Maven  3.8.0")?;
    ui.label(&mut app, "修改")?;
    ui.label(&mut app, "MAVEN")?;
    ui.label(&mut app, "GRADLE")?;
    ui.fill(&mut app, "如：Maven 3.9", "fixture Gradle")?;
    ui.fill(&mut app, "构建工具主目录", &tools.to_string_lossy())?;
    ui.fill(&mut app, "自动识别，也可以手动填写", "9.5.0")?;
    ui.label(&mut app, "保存")?;
    ui.until(&mut app, |s| s.0.text("fixture Gradle  9.5.0").is_ok())?;
    ui.label(&mut app, "项目配置")?;
    ui.until(&mut app, |s| s.0.text("未指定 Maven").is_ok())?;
    ui.label(&mut app, "确定")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.development.as_ref().unwrap().probe().settings_open {
        ui.frame(&mut app, vec![]);
        anyhow::ensure!(Instant::now() < deadline, "build tool was not saved");
        std::thread::sleep(Duration::from_millis(10));
    }
    let (saved, _) =
        tomato_services::development::repository::load(&root.join("development.json"))?;
    assert_eq!(saved.build_tools.len(), 1);
    let tool = &saved.build_tools[0];
    assert_eq!(tool.id, "legacy-tool");
    assert_eq!(tool.kind, "GRADLE");
    assert_eq!(tool.name, "fixture Gradle");
    assert_eq!(tool.version, "9.5.0");
    assert!(tool.settings_path.is_empty());
    assert_eq!(tool.extra["futureToolField"]["keep"], true);
    assert!(saved.projects[0].maven_id.is_empty());
    assert_eq!(
        std::fs::read(root.join("development.json.before-rust.bak"))?,
        original
    );
    let after_save = std::fs::read(root.join("development.json"))?;
    open_settings(&mut app)?;
    ui.label(&mut app, "构建管理")?;
    ui.label(&mut app, "fixture Gradle  9.5.0")?;
    ui.label(&mut app, "删除")?;
    ui.label(&mut app, "取消")?;
    assert_eq!(std::fs::read(root.join("development.json"))?, after_save);
    assert!(tools.is_dir());
    assert!(!root.join("connections.json").exists());
    app.shutdown_file_transfers();
    Ok(())
}
