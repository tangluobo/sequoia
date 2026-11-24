//! Run the system print workflow outside the UI frame with one frozen draft.
use super::*;

#[cfg(windows)]
mod fixture;
#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod worker;

#[cfg(windows)]
pub(crate) fn install(ctx: &egui::Context, render: &egui_wgpu::RenderState) {
    ctx.data_mut(|data| {
        data.insert_temp(
            graphics_id(),
            gpu::Graphics {
                device: render.device.clone(),
                queue: render.queue.clone(),
            },
        )
    });
}

#[cfg(windows)]
fn graphics_id() -> egui::Id {
    egui::Id::new("tomato-markdown-print-graphics")
}

pub(super) enum State {
    #[cfg(windows)]
    Pending {
        control: tomato_services::backup::Control,
        receiver: tokio::sync::oneshot::Receiver<anyhow::Result<Option<worker::Report>>>,
    },
    Notice {
        success: bool,
        message: String,
    },
}

impl TomatoApp {
    pub(super) fn start_markdown_print(&mut self, _id: &str) {
        if self.markdown_print.is_some() {
            return;
        }
        #[cfg(windows)]
        if let Err(error) = self.queue_markdown_print(_id) {
            self.markdown_print = Some(State::Notice {
                success: false,
                message: error.to_string(),
            });
        }
        #[cfg(not(windows))]
        {
            self.markdown_print = Some(State::Notice {
                success: false,
                message: "当前系统尚未接入 Markdown 打印".into(),
            });
        }
    }

    #[cfg(windows)]
    fn queue_markdown_print(&mut self, id: &str) -> anyhow::Result<()> {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return Ok(());
        };
        let Panel::Editor {
            path,
            text,
            markdown,
            ..
        } = &tab.panel
        else {
            return Ok(());
        };
        worker::validate_source(text)?;
        let graphics = self
            .context
            .data(|data| data.get_temp::<gpu::Graphics>(graphics_id()))
            .ok_or_else(|| anyhow::anyhow!("打印渲染器不可用"))?;
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("后台运行时不可用"))?;
        let request = worker::Request {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| tab.title.clone()),
            source: text.clone(),
            document: path.clone(),
            remote: markdown.remote.clone(),
            client: markdown.file_client.clone(),
            fonts: self.context.fonts(|fonts| fonts.definitions().clone()),
            owner: tomato_native_print::Owner::active(),
            graphics,
            fixture: fixture::destination(&self.context),
        };
        let ctx = self.context.clone();
        let control = tomato_services::backup::Control::new({
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        let (send, receiver) = tokio::sync::oneshot::channel();
        let task = control.clone();
        runtime.spawn(async move {
            let result = worker::run(request, &task).await;
            let _ = send.send(result);
            ctx.request_repaint();
        });
        self.markdown_print = Some(State::Pending { control, receiver });
        Ok(())
    }

    pub(super) fn markdown_print_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        fixture::poll(self, ctx);
        #[cfg(windows)]
        if let Some(State::Pending { control, receiver }) = self.markdown_print.as_mut() {
            use tokio::sync::oneshot::error::TryRecvError;
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Closed) => Some(Err(anyhow::anyhow!("打印任务意外结束"))),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.markdown_print = match result {
                    Ok(Some(report)) => Some(State::Notice {
                        success: true,
                        message: report.message(),
                    }),
                    _ if control.cancelled() => None,
                    Ok(None) => {
                        fixture::complete(ctx, true, true, "已取消系统打印");
                        None
                    }
                    Err(error) => Some(State::Notice {
                        success: false,
                        message: format!("{error:#}"),
                    }),
                };
            }
        }
        let Some(state) = self.markdown_print.as_ref() else {
            return;
        };
        #[cfg(windows)]
        if let State::Notice { success, message } = state {
            fixture::complete(ctx, *success, false, message);
        }
        let dialog = egui::Modal::new(egui::Id::new("markdown-print-dialog"))
            .backdrop_color(Color32::TRANSPARENT)
            .show(ctx, |ui| {
                ui.set_width(326.);
                match state {
                    #[cfg(windows)]
                    State::Pending { control, .. } => {
                        ui.heading("导出 PDF");
                        if control.cancelled() {
                            ui.label("正在取消打印…");
                        } else if let Some((sent, total)) =
                            control.read(|progress| (progress.objects, progress.total))
                            && total != 0
                        {
                            ui.label(format!("正在发送页面 {sent}/{total}…"));
                        } else {
                            ui.label("正在准备文档，请在系统打印对话框中选择打印机。");
                        }
                        if ui
                            .add_enabled(!control.cancelled(), egui::Button::new("取消"))
                            .clicked()
                        {
                            control.cancel();
                        }
                        false
                    }
                    State::Notice { success, message } => {
                        ui.heading(if *success {
                            "导出成功"
                        } else {
                            "导出失败"
                        });
                        ui.add(egui::Label::new(message).wrap().selectable(true));
                        ui.button("确定").clicked()
                    }
                }
            });
        if dialog.inner
            || (dialog.is_top_modal
                && ctx
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
        {
            match state {
                #[cfg(windows)]
                State::Pending { control, .. } => control.cancel(),
                State::Notice { .. } => self.markdown_print = None,
            }
        }
    }

    pub(super) fn cancel_markdown_print(&mut self) {
        #[cfg(windows)]
        if let Some(State::Pending { control, .. }) = &self.markdown_print {
            control.cancel();
        }
        self.markdown_print = None;
    }
}
