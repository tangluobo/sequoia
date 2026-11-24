use super::*;
use tomato_services::files::object_copy::{Job, Owner, Phase, Snapshot};

pub struct Upload {
    pub owner: Owner,
    pub positioned: bool,
    // Some integrations own temporary drop payloads through these handles.
    _files: Vec<egui::DroppedFileHandle>,
}
impl Workspace {
    pub fn upload_files(
        &mut self,
        ctx: &egui::Context,
        connection: &tomato_core::Connection,
        directory: String,
        paths: Vec<std::path::PathBuf>,
        files: Vec<egui::DroppedFileHandle>,
    ) {
        let deferred = files
            .iter()
            .filter_map(|file| file.deferred_drop())
            .map(|(retained, stage)| tomato_services::files::upload::Deferred::new(retained, stage))
            .collect::<Vec<_>>();
        let error = if paths.is_empty() && deferred.is_empty() {
            return;
        } else if paths.len() > 1000 {
            Some("一次最多上传 1000 个文件或目录")
        } else if tomato_services::files::object_copy::is_object(connection)
            && connection.text("bucket").is_empty()
            && directory.trim_matches('/').is_empty()
        {
            Some("请先进入一个 Bucket 再上传文件")
        } else if self.file_status != Status::Ready {
            Some("未连接，无法上传")
        } else if self.upload.is_some() {
            Some("已有文件上传任务正在执行")
        } else {
            None
        };
        if let Some(error) = error {
            self.panes.entry(Section::Files).or_default().status = error.into();
            return;
        }
        let job = std::sync::Arc::new(Job::default());
        let context = ctx.clone();
        job.on_change(move || context.request_repaint());
        let before = self.queued.len();
        self.enqueue(
            Section::Files,
            Operation::UploadFiles {
                directory,
                paths,
                deferred,
                job: job.clone(),
            },
        );
        if self.queued.len() > before {
            self.upload = Some(Box::new(Upload {
                owner: Owner(job),
                positioned: false,
                _files: files,
            }));
            self.upload_report = None;
            self.panes.entry(Section::Files).or_default().status = "上传中...".into();
        }
    }
}
pub(super) fn notice(report: &Snapshot) -> String {
    if report.phase == Phase::Failed {
        format!("读取上传目录失败: {}", report.current)
    } else if report.phase == Phase::Cancelled {
        format!("上传已取消: 已完成 {} 个", report.succeeded)
    } else if report.failed == 0 {
        format!("上传完成: 成功 {} 个", report.succeeded)
    } else {
        format!(
            "上传结束: 成功 {} 个, 失败 {} 个",
            report.succeeded, report.failed
        )
    }
}
