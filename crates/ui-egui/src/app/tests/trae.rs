use super::*;
use tomato_services::trae::{Action, Mutation};
#[test]
fn real_trae_controller_keeps_failed_name_drafts_and_never_touches_connection_config()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("connections.json");
    Store::default().save(&config)?;
    let before = std::fs::read(&config)?;
    let path = root.path().join("trae_sessions.json");
    let user = root.path().join("User");
    let archive = root.path().join("User111");
    std::fs::create_dir(&archive)?;
    std::fs::write(
        &path,
        serde_json::to_vec(
            &serde_json::json!({"traeUserDir":user,"sessions":[{"phone":"111","name":"旧名称","active":false}]}),
        )?,
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&config));
    app.section = 1;
    app.open_tool("trae_session", "Trae 会话");
    let id = app.tabs[0].id.clone();
    if let Panel::Tool(tool) = &mut app.tabs[0].panel {
        tool.trae.get_or_insert_with(Default::default).path = Some(path.clone());
    }
    let pump = |app: &mut TomatoApp| -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.logic(&ctx);
            render_frame(&ctx, app);
            if !app.tabs[0].busy
                && matches!(&app.tabs[0].panel,Panel::Tool(tool) if tool.trae.as_ref().is_some_and(|v|v.snapshot.is_some() && !v.pending_tasks))
            {
                return Ok(());
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Trae command controller timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    pump(&mut app)?;
    let snapshot = match &mut app.tabs[0].panel {
        Panel::Tool(tool) => {
            let view = tool.trae.as_mut().unwrap();
            assert_eq!(view.selected.as_deref(), Some("111"));
            assert_eq!(view.task_status, "未找到 workspaceStorage 目录");
            view.dialog = Some(crate::tabs::trae::Dialog::Rename {
                phone: "111".into(),
                name: "新名称草稿".into(),
            });
            view.snapshot.as_ref().unwrap().as_ref().clone()
        }
        _ => unreachable!(),
    };
    let action = Action::Mutate {
        previous: Box::new(snapshot),
        mutation: Mutation::Rename {
            phone: "111".into(),
            name: "新名称草稿".into(),
        },
    };
    let external = std::fs::read_to_string(&path)?.replace("旧名称", "外部改名");
    std::fs::write(&path, &external)?;
    app.handle_intent(
        &id,
        Intent::Command(Box::new(Command::Trae {
            path: path.clone(),
            action,
        })),
    );
    pump(&mut app)?;
    assert!(app.tabs[0].error.is_some());
    assert!(
        matches!(&app.tabs[0].panel,Panel::Tool(tool) if matches!(tool.trae.as_ref().unwrap().dialog,Some(crate::tabs::trae::Dialog::Rename {ref name,..}) if name=="新名称草稿"))
    );
    assert_eq!(std::fs::read_to_string(path)?, external);
    assert_eq!(std::fs::read(config)?, before);
    assert!(archive.is_dir());
    app.close(&id);
    assert!(app.tabs.is_empty());
    Ok(())
}
