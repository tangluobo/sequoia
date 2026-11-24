use super::*;
use tokio::sync::oneshot;
use tomato_services::{
    commands::{CommandHandler, Engine},
    remote_desktop::AdbPairing,
};

#[derive(Default)]
pub(in crate::app) struct Pairing {
    address: String,
    code: String,
    job: Option<Job>,
    outcome: Option<Result<(), String>>,
}

struct Job {
    worker: tokio::task::AbortHandle,
    outcome: oneshot::Receiver<Result<(), String>>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

impl Pairing {
    pub(in crate::app) fn show(&mut self, ui: &mut Ui, runtime: Option<&tokio::runtime::Handle>) {
        if let Some(job) = &mut self.job {
            match job.outcome.try_recv() {
                Ok(outcome) => {
                    self.outcome = Some(outcome);
                    self.job = None;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.outcome = Some(Err("配对任务已结束".into()));
                    self.job = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        ui.label(RichText::new("连接端口以 Android 无线调试主页面为准。").size(11.));
        ui.collapsing("无线配对", |ui| {
            ui.label("在 Android 上打开「使用配对码配对设备」，填写弹窗中的地址和六位配对码。");
            let enabled = self.job.is_none();
            ui.add_enabled_ui(enabled, |ui| {
                egui::Grid::new("adb-wireless-pairing")
                    .spacing([10., 8.])
                    .show(ui, |ui| {
                        ui.label("配对地址");
                        ui.add(TextEdit::singleline(&mut self.address).hint_text("IP:配对端口"));
                        ui.end_row();
                        ui.label("配对码");
                        ui.add(
                            TextEdit::singleline(&mut self.code)
                                .password(true)
                                .char_limit(6),
                        );
                        ui.end_row();
                    });
            });
            if self.job.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("正在配对…");
                    if ui.button("取消配对").clicked() {
                        self.job = None;
                    }
                });
            } else if let Some(runtime) = runtime
                && ui
                    .add_enabled(
                        !self.address.trim().is_empty() && self.code.len() == 6,
                        egui::Button::new("配对"),
                    )
                    .clicked()
            {
                let request = AdbPairing::new(self.address.clone(), std::mem::take(&mut self.code));
                let repaint = ui.ctx().clone();
                let (send, outcome) = oneshot::channel();
                let worker = runtime.spawn(async move {
                    let result = Engine::default()
                        .execute(Command::AdbPair(request))
                        .await
                        .map(|_| ())
                        .map_err(|error| format!("{error:#}"));
                    let _ = send.send(result);
                    repaint.request_repaint();
                });
                self.outcome = None;
                self.job = Some(Job {
                    worker: worker.abort_handle(),
                    outcome,
                });
            }
            match &self.outcome {
                Some(Ok(())) => {
                    ui.label("配对成功，保存连接后即可打开远程屏幕。");
                }
                Some(Err(error)) => {
                    ui.colored_label(Color32::DARK_RED, error);
                }
                None => {}
            }
        });
    }
}
