use super::*;
use crate::app::tests::native_bridge::Bridge;

struct Fixture {
    app: TomatoApp,
    context: egui::Context,
    file: PathBuf,
    _directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("connections.json");
        let kinds = ["SFTP", "FTP", "S3", "ALIYUN_OSS"];
        let connections: Vec<_> = kinds
            .into_iter()
            .map(|kind| {
                let mut connection = Connection::new(kind);
                connection.name = format!("文件标签-{kind}");
                connection.set("futureField", serde_json::json!({"keep":[1,2]}));
                connection
            })
            .collect();
        Store {
            connections: connections.clone(),
            ..Default::default()
        }
        .save(&file)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let context = egui::Context::default();
        let mut app =
            TomatoApp::with_repository(&context, runtime, ConnectionFileRepository::new(&file));
        for connection in connections {
            // Inert owned local transport under each actual file-browser view;
            // configuration gestures must not connect to saved or external hosts.
            let mut transport = Connection::new("LOCAL_DIRECTORY");
            transport.set(
                "localDirectoryPath",
                directory.path().to_string_lossy().as_ref(),
            );
            let mut tab =
                tabs::create_tab(transport, Some(app.runtime.as_ref().unwrap().handle()))?;
            tab.source_id = connection.id.clone();
            tab.title = connection.name.clone();
            if let Panel::Terminal {
                connection: live,
                view,
                ..
            } = &mut tab.panel
            {
                *live = connection;
                view.ssh = Some(Box::new(crate::tabs::ssh::Workspace::standalone_files(
                    "LIST", "/current",
                )));
            }
            terminal_config::apply(&mut tab, &app.settings);
            app.tabs.push(tab);
        }
        app.active = Some(app.tabs[3].id.clone());
        Ok(Self {
            app,
            context,
            file,
            _directory: directory,
        })
    }
}

#[test]
fn standalone_file_menu_targets_inactive_tab_refreshes_current_directory_and_opens_session()
-> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    for index in 0..4 {
        let before = std::fs::read(&f.file)?;
        let active = f.app.active.clone();
        let id = f.app.tabs[index].id.clone();
        assert!(supports(&f.app.tabs[index]));
        f.app.file_tab_action(&id, Action::Refresh);
        let Panel::Terminal { view, .. } = &mut f.app.tabs[index].panel else {
            unreachable!()
        };
        let pane = view
            .ssh
            .as_mut()
            .unwrap()
            .panes
            .get_mut(&Section::Files)
            .unwrap();
        assert!(pane.refresh);
        assert_eq!(pane.path, "/current");
        f.app.file_tab_action(&id, Action::Session);
        assert!(f.app.session_config.is_some());
        assert_eq!(f.app.active, active);
        let bridge = Bridge::new(&f.context);
        bridge.frame(&f.context, vec![], |ctx| f.app.session_config_dialog(ctx));
        assert!(bridge.text("默认文件视图:").is_ok());
        bridge.click(&f.context, bridge.text("取消")?, |ctx| {
            f.app.session_config_dialog(ctx)
        });
        assert_eq!(std::fs::read(&f.file)?, before);
    }
    Ok(())
}

#[test]
fn file_session_modes_apply_live_save_encrypted_and_preserve_original_defaults_and_unknown_fields()
-> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    for (index, expected) in ["ICON", "LIST", "ICON", "ICON"].into_iter().enumerate() {
        let Panel::Terminal { view, .. } = &f.app.tabs[index].panel else {
            unreachable!()
        };
        assert_eq!(view.ssh.as_ref().unwrap().file_view, expected);
        let id = f.app.tabs[index].id.clone();
        f.app.file_tab_action(&id, Action::Session);
        let bridge = Bridge::new(&f.context);
        bridge.frame(&f.context, vec![], |ctx| f.app.session_config_dialog(ctx));
        let inherit = egui::pos2(260., bridge.text("默认文件视图:")?.y);
        bridge.click(&f.context, inherit, |ctx| f.app.session_config_dialog(ctx));
        bridge.click(&f.context, bridge.text("详细列表")?, |ctx| {
            f.app.session_config_dialog(ctx)
        });
        let (choice, mode) = if index == 1 {
            ("图标视图", "ICON")
        } else {
            ("多列列表", "COLUMN")
        };
        bridge.click(&f.context, bridge.text(choice)?, |ctx| {
            f.app.session_config_dialog(ctx)
        });
        bridge.click(&f.context, bridge.text("确定")?, |ctx| {
            f.app.session_config_dialog(ctx)
        });
        let saved = Store::load(&f.file)?;
        assert_eq!(saved.connections[index].text("defaultFileViewMode"), mode);
        assert_eq!(
            saved.connections[index].options["futureField"],
            serde_json::json!({"keep":[1,2]})
        );
        let Panel::Terminal { view, .. } = &f.app.tabs[index].panel else {
            unreachable!()
        };
        assert_eq!(view.ssh.as_ref().unwrap().file_view, mode);
    }
    assert!(!String::from_utf8_lossy(&std::fs::read(&f.file)?).contains("futureField"));
    Ok(())
}
