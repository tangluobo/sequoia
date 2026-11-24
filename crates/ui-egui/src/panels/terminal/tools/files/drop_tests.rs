use super::*;
use std::sync::Arc;
#[derive(Debug)]
struct File(std::path::PathBuf);
impl egui::DroppedFile for File {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        panic!("file contents must not be loaded in the UI frame")
    }
}
fn frame(
    ctx: &egui::Context,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    point: egui::Pos2,
    files: Vec<egui::DroppedFileHandle>,
    enabled: bool,
) {
    let retry = !files.is_empty();
    let mut discarded = false;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 800.),
            )),
            events: vec![egui::Event::PointerMoved(point), egui::Event::PointerGone],
            dropped_files: files,
            ..Default::default()
        },
        |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let rect = ui.max_rect();
                super::super::render(
                    ui,
                    rect,
                    workspace,
                    connection,
                    &mut crate::icons::Icons::default(),
                );
            });
            if retry && !discarded {
                discarded = true;
                ui.ctx().request_discard("exercise file drop replay");
            }
        },
    );
    output.textures_delta.clear();
}
#[test]
fn file_drop_uploads_from_full_empty_views_once_without_confirmation_and_owns_payload_until_cancel()
{
    for protocol in ["S3", "ALIYUN_OSS", "SFTP", "SSH", "FTP", "LOCAL_DIRECTORY"] {
        for mode in ["LIST", "ICON", "COLUMN"] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let connection = tomato_core::Connection::new(protocol);
            let mut workspace = Workspace::standalone_files(mode, "/fixture");
            workspace.file_status = tomato_services::terminal::Status::Ready;
            let file: egui::DroppedFileHandle = Arc::new(File("C:/owned-fixture/中文.txt".into()));
            frame(
                &ctx,
                &mut workspace,
                &connection,
                egui::pos2(600., 700.),
                vec![],
                true,
            );
            frame(
                &ctx,
                &mut workspace,
                &connection,
                egui::pos2(600., 700.),
                vec![file.clone()],
                true,
            );
            let job = workspace
                .upload
                .as_ref()
                .unwrap_or_else(|| {
                    panic!("{protocol}/{mode} did not accept drop in empty file area")
                })
                .owner
                .0
                .clone();
            assert!(workspace.dialog.is_none());
            assert_eq!(
                Arc::strong_count(&file),
                2,
                "drop payload must live until completion"
            );
            assert_eq!(workspace.panes[&Section::Files].status, "上传中...");
            frame(
                &ctx,
                &mut workspace,
                &connection,
                egui::pos2(600., 700.),
                vec![],
                true,
            );
            assert!(Arc::ptr_eq(
                &workspace.upload.as_ref().unwrap().owner.0,
                &job
            ));
            workspace.upload = None;
            assert!(job.is_cancelled());
            assert_eq!(Arc::strong_count(&file), 1);
        }
    }
}

#[test]
fn virtual_file_drop_queues_ssh_upload_without_staging_in_the_frame() {
    #[derive(Debug)]
    struct Virtual;
    impl egui::DroppedFile for Virtual {
        fn path(&self) -> &std::path::Path {
            std::path::Path::new("")
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            panic!("file contents must not be loaded in the UI frame")
        }
        fn deferred_drop(&self) -> Option<egui::DeferredFileDrop> {
            Some((64, Arc::new(|| panic!("ZIP must be staged on a worker"))))
        }
    }

    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let connection = tomato_core::Connection::new("SSH");
    let mut workspace = Workspace::standalone_files("LIST", "/fixture");
    workspace.file_status = tomato_services::terminal::Status::Ready;
    let point = egui::pos2(600., 700.);
    frame(&ctx, &mut workspace, &connection, point, vec![], true);
    frame(
        &ctx,
        &mut workspace,
        &connection,
        point,
        vec![Arc::new(Virtual)],
        true,
    );
    assert!(workspace.upload.is_some());
}
#[test]
fn file_drop_rejects_disabled_disconnected_bucket_root_and_outside_area_without_uploading() {
    for (protocol, path, ready, enabled, point, expected) in [
        (
            "S3",
            "/",
            true,
            true,
            egui::pos2(600., 700.),
            Some("请先进入一个 Bucket 再上传文件"),
        ),
        (
            "ALIYUN_OSS",
            "/",
            true,
            true,
            egui::pos2(600., 700.),
            Some("请先进入一个 Bucket 再上传文件"),
        ),
        (
            "SFTP",
            "/fixture",
            false,
            true,
            egui::pos2(600., 700.),
            Some("未连接，无法上传"),
        ),
        ("S3", "/fixture", true, false, egui::pos2(600., 700.), None),
        ("S3", "/fixture", true, true, egui::pos2(-10., 700.), None),
    ] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut workspace = Workspace::standalone_files("LIST", path);
        if ready {
            workspace.file_status = tomato_services::terminal::Status::Ready;
        }
        let connection = tomato_core::Connection::new(protocol);
        frame(&ctx, &mut workspace, &connection, point, vec![], enabled);
        frame(
            &ctx,
            &mut workspace,
            &connection,
            point,
            vec![Arc::new(File("C:/owned-fixture/file.txt".into()))],
            enabled,
        );
        assert!(workspace.upload.is_none(), "{protocol}/{path}");
        if let Some(expected) = expected {
            assert_eq!(workspace.panes[&Section::Files].status, expected);
        }
    }
}
