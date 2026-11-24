//! Real file actor replies cannot overwrite the native outcome or a newer drag.
use super::*;
use tomato_services::files::export::Request;

fn export(request: Request) -> Box<FileExport> {
    Box::new(FileExport {
        path: request.path.clone(),
        cancel: request.cancel.clone(),
        asset: None,
        gesture: true,
        request,
        native: None,
    })
}
#[test]
fn late_cancel_preserves_native_failure_and_a_new_drag_of_the_same_path() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_string_lossy().replace('\\', "/");
    let mut connection = tomato_core::Connection::new("LOCAL_DIRECTORY");
    connection.set("localDirectoryPath", path.as_str());
    for retry in [false, true] {
        let mut session = Terminal::start(&connection, runtime.handle())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.status() != Status::Ready {
            assert!(
                Instant::now() < deadline,
                "file actor did not become ready: {:?}",
                session.status()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let ctx = egui::Context::default();
        let mut workspace = Workspace::standalone_files("LIST", &path);
        workspace.panes.get_mut(&Section::Files).unwrap().refresh = false;
        let request = Request::new(format!("{path}/cancelled.txt"));
        request.cancel.cancel();
        workspace.panes.get_mut(&Section::Files).unwrap().export = Some(export(request.clone()));
        workspace.enqueue(Section::Files, Operation::ExportFile(request));
        workspace.poll(&mut session, &ctx, true);
        let replacement = Request::new(format!("{path}/cancelled.txt"));
        let pane = workspace.panes.get_mut(&Section::Files).unwrap();
        pane.export = retry.then(|| export(replacement.clone()));
        let expected = if retry {
            "新的拖放准备中"
        } else {
            "系统文件拖放失败：native fixture"
        };
        pane.status = expected.into();
        while workspace.busy(Section::Files) {
            assert!(Instant::now() < deadline, "cancel reply was not consumed");
            workspace.poll(&mut session, &ctx, true);
            std::thread::sleep(Duration::from_millis(5));
        }
        let pane = &workspace.panes[&Section::Files];
        assert_eq!(pane.status, expected);
        assert_eq!(
            pane.export
                .as_ref()
                .is_some_and(|active| active.request.same_job(&replacement)),
            retry
        );
        runtime.block_on(session.shutdown());
    }
    Ok(())
}
