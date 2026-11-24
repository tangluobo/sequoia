//! Native picker, background file job and the original export alert.
use super::*;
use crate::native_dialog::message::{self, Button, Kind, Message, Role};
#[cfg(windows)]
mod fixture;

pub(super) enum State {
    Writing(u64),
    Notice {
        success: bool,
        message: String,
        focus: bool,
    },
}

impl State {
    pub(super) fn modal(&self) -> bool {
        matches!(self, Self::Notice { .. })
    }

    fn notice(success: bool, message: String) -> Self {
        Self::Notice {
            success,
            message,
            focus: true,
        }
    }
}

impl TomatoApp {
    pub(super) fn start_markdown_html(&mut self, id: &str) {
        if self.markdown_html.is_some() {
            return;
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id && !tab.busy) else {
            return;
        };
        let Panel::Editor { text, .. } = &tab.panel else {
            return;
        };
        if text.len() > tomato_services::markdown::MAX_SOURCE_BYTES {
            self.markdown_html = Some(State::notice(false, "Markdown 最多 8 MiB".into()));
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("导出 HTML")
            .add_filter("HTML 文件", &["html"])
            .set_file_name(tomato_services::markdown::html_filename(&tab.title))
            .save_file()
        else {
            #[cfg(windows)]
            fixture::complete(&self.context, false, true, "");
            return;
        };
        let command = Command::MarkdownHtml {
            source: text.clone(),
            title: tab.title.clone(),
            path,
        };
        match self.queue.submit(command) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: id.into(),
                        purpose: Purpose::MarkdownHtml,
                    },
                );
                self.markdown_html = Some(State::Writing(task));
            }
            Err(error) => self.markdown_html = Some(State::notice(false, error.to_string())),
        }
    }

    pub(super) fn markdown_html_reply(&mut self, task: u64, result: anyhow::Result<Output>) {
        if !matches!(self.markdown_html, Some(State::Writing(id)) if id == task) {
            return;
        }
        let state = match result {
            Ok(Output::Text(message)) => State::notice(true, message),
            Ok(_) => State::notice(false, "导出返回了未知结果".into()),
            Err(error) => State::notice(false, format!("{error:#}")),
        };
        self.markdown_html = Some(state);
    }

    pub(super) fn markdown_html_dialog(&mut self, ctx: &egui::Context) {
        let Some(State::Notice {
            success,
            message,
            focus,
        }) = &mut self.markdown_html
        else {
            return;
        };
        let id = egui::ViewportId::from_hash_of("markdown-html-alert");
        #[cfg(windows)]
        {
            fixture::select_viewport(ctx, id);
            fixture::complete(ctx, *success, false, message);
        }
        let answer = message::show(
            ctx,
            Message {
                id,
                owner: ctx.input(|input| input.viewport().inner_rect),
                title: if *success {
                    "导出成功"
                } else {
                    "导出失败"
                },
                header: None,
                content: message,
                kind: if *success {
                    Kind::Information
                } else {
                    Kind::Error
                },
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            focus,
            &mut self.icons,
            |_ui, _| {
                #[cfg(windows)]
                fixture::capture(_ui, id);
            },
        );
        if answer.is_some() {
            self.markdown_html = None;
        }
    }
}
