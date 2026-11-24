use super::*;
use std::{sync::Arc, time::Instant};
use tomato_services::files::object_copy::{Job, Owner};

fn fixture() -> anyhow::Result<(tempfile::TempDir, TomatoApp)> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let app = TomatoApp::with_repository(
        &egui::Context::default(),
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    Ok((directory, app))
}
fn close_input() -> egui::RawInput {
    let mut raw = egui::RawInput::default();
    raw.viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    raw
}
fn commands(output: &egui::LogicOutput) -> &[egui::ViewportCommand] {
    output
        .viewport_commands
        .get(&egui::ViewportId::ROOT)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
#[test]
fn closing_remains_responsive_and_idempotent_while_upload_guards_are_cleaning_up()
-> anyhow::Result<()> {
    let (directory, mut app) = fixture()?;
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    connection.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut tab = tabs::create_tab(connection, app.runtime.as_ref().map(|r| r.handle()))?;
    let Panel::Terminal { view, .. } = &mut tab.panel else {
        panic!("file session")
    };
    let job = Arc::new(Job::default());
    view.ssh.as_mut().unwrap().object_copy = Some(Owner(job.clone()));
    app.tabs.push(tab);
    let ctx = app.context.clone();
    let started = Instant::now();
    let output = ctx.run_logic(&close_input(), |ctx| app.logic(ctx));
    assert!(started.elapsed() < Duration::from_millis(200));
    assert!(commands(&output).contains(&egui::ViewportCommand::CancelClose));
    assert!(app.closing() && job.is_cancelled() && app.tabs.is_empty());
    // Hold the actual cleanup future at its completion boundary to exercise
    // close/repaint events while an asynchronous resource is still retiring.
    let (release, wait) = tokio::sync::oneshot::channel();
    let mut shutdown = app.shutdown.take().unwrap();
    let cleanup = shutdown.task;
    shutdown.task = app.runtime.as_ref().unwrap().spawn(async move {
        let _ = cleanup.await;
        let _ = wait.await;
    });
    app.shutdown = Some(shutdown);
    let first_task = app.shutdown.as_ref().unwrap().task.id();
    for _ in 0..3 {
        app.begin_shutdown();
        let output = ctx.run_logic(&close_input(), |ctx| app.logic(ctx));
        assert!(commands(&output).contains(&egui::ViewportCommand::CancelClose));
        assert!(!commands(&output).contains(&egui::ViewportCommand::Close));
        assert_eq!(first_task, app.shutdown.as_ref().unwrap().task.id());
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui));
        assert!(!frame.shapes.is_empty(), "shutdown UI still paints");
        frame.textures_delta.clear();
    }
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        if commands(&output).contains(&egui::ViewportCommand::Close) {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "cleanup did not complete");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
#[test]
fn unsaved_documents_do_not_start_shutdown_until_exit_is_approved() -> anyhow::Result<()> {
    let (directory, mut app) = fixture()?;
    app.tabs.push(Tab {
        id: "dirty".into(),
        source_id: "document".into(),
        title: "未保存".into(),
        panel: Panel::Editor {
            path: directory.path().join("note.md"),
            text: "new".into(),
            saved: String::new(),
            markdown: Box::default(),
        },
        busy: false,
        error: None,
    });
    let ctx = app.context.clone();
    let output = ctx.run_logic(&close_input(), |ctx| app.logic(ctx));
    assert!(commands(&output).contains(&egui::ViewportCommand::CancelClose));
    assert!(matches!(app.confirmation, Some(Confirmation::Quit)));
    assert!(!app.closing() && app.tabs.len() == 1);
    app.quit_approved = true;
    let _ = ctx.run_logic(&close_input(), |ctx| app.logic(ctx));
    assert!(app.closing());
    Ok(())
}
