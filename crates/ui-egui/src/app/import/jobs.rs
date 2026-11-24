use super::*;
use tomato_services::{
    backup::Control,
    connections::{Output as Imported, Request},
};

pub(in crate::app) struct Reader {
    id: u64,
    control: Control,
    focus: bool,
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl TomatoApp {
    pub(in crate::app) fn read_connection_import(&mut self, source: PathBuf) {
        if self.closing() || self.import_reader.is_some() || self.import_dialog.is_some() {
            return;
        }
        let result = (|| -> anyhow::Result<_> {
            let baseline = tomato_core::config::ImportBaseline::new(&self.store)?;
            let control = Control::default();
            let id = self.queue.submit(Command::ConnectionImport {
                request: Request::Preview { source, baseline },
                control: Some(control.clone()),
            })?;
            Ok((id, control))
        })();
        match result {
            Ok((id, control)) => {
                self.import_reader = Some(Reader {
                    id,
                    control,
                    focus: true,
                });
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::ConnectionImportRead,
                    },
                );
            }
            Err(error) => self.error = Some(format!("导入失败：{error:#}")),
        }
    }
    pub(super) fn import_read_dialog(&mut self, ctx: &egui::Context) {
        use crate::native_dialog::message::{self, Button, Kind, Role};
        let Some(reader) = &mut self.import_reader else {
            return;
        };
        if message::show(
            ctx,
            message::Message {
                id: egui::ViewportId::from_hash_of("connection-import-read"),
                owner: ctx.input(|i| i.viewport().inner_rect),
                title: "导入连接",
                header: None,
                content: "正在读取连接文件…",
                kind: Kind::Information,
                buttons: &[Button {
                    label: "取消",
                    role: Role::Cancel,
                }],
                active: true,
            },
            &mut reader.focus,
            &mut self.icons,
            |_, _| {},
        )
        .is_some()
        {
            self.import_reader = None;
        }
    }
    pub(super) fn commit_connection_import(&mut self) {
        let Some(dialog) = &mut self.import_dialog else {
            return;
        };
        let result = (|| -> anyhow::Result<_> {
            anyhow::ensure!(
                !self.load_failed,
                "配置加载失败，保存已禁用。请修复原配置后重启。"
            );
            dialog.plan.matches_existing(&self.store)?;
            let guard = self
                .repository
                .save_guard()?
                .ok_or_else(|| anyhow::anyhow!("此配置仓库不支持后台导入"))?;
            let source = dialog
                .source
                .clone()
                .ok_or_else(|| anyhow::anyhow!("缺少导入文件"))?;
            let control = Control::default();
            let id = self.queue.submit(Command::ConnectionImport {
                request: Request::Commit {
                    source,
                    source_signature: dialog.plan.source_signature(),
                    guard,
                    selected: dialog.selected.clone(),
                    prepared: Some(dialog.plan.clone()),
                },
                control: Some(control.clone()),
            })?;
            Ok((id, control))
        })();
        match result {
            Ok((id, control)) => {
                dialog.phase = Phase::Writing {
                    id,
                    control,
                    canceling: false,
                };
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::ConnectionImportCommit,
                    },
                );
            }
            Err(error) => dialog.notice(
                format!("导入失败：{error:#}"),
                crate::native_dialog::message::Kind::Error,
            ),
        }
    }
    pub(in crate::app) fn connection_import_reply(
        &mut self,
        id: u64,
        result: anyhow::Result<Output>,
    ) {
        if self
            .import_reader
            .as_ref()
            .is_some_and(|reader| reader.id == id)
        {
            self.import_reader = None;
            match result {
                Ok(Output::ConnectionImport(Imported::Preview { source, plan, .. })) => {
                    if let Err(error) = plan.matches_existing(&self.store) {
                        self.error = Some(format!("导入失败：{error:#}"));
                        return;
                    }
                    self.import_dialog = Some(Dialog::from_shared(plan, Some(source)));
                }
                Err(error) => self.error = Some(format!("导入失败：{error:#}")),
                _ => self.error = Some("导入失败：返回结果类型不正确".into()),
            }
            return;
        }
        let Some(dialog) = &mut self.import_dialog else {
            return;
        };
        let Phase::Writing {
            id: active,
            canceling,
            ..
        } = &dialog.phase
        else {
            return;
        };
        if *active != id {
            return;
        }
        let canceled = *canceling;
        match result {
            Ok(Output::ConnectionImport(Imported::Committed {
                count,
                added,
                receipt,
            })) => {
                let accept = dialog
                    .plan
                    .matches_existing(&self.store)
                    .and_then(|()| self.repository.accept_saved(receipt));
                if let Err(error) = accept {
                    dialog.notice(
                        format!("导入已完成磁盘写入，请重新加载连接配置：{error:#}"),
                        crate::native_dialog::message::Kind::Error,
                    );
                    return;
                }
                self.store.connections.extend(added);
                self.status = format!("已成功导入 {count} 个项目");
                if canceled {
                    self.import_dialog = None;
                } else {
                    dialog.phase = Phase::Done(self.status.clone());
                    dialog.notice_focus = true;
                }
                self.collaboration_connections_changed();
            }
            Err(_) if canceled => self.import_dialog = None,
            Err(error) => dialog.notice(
                format!("导入失败：{error:#}"),
                crate::native_dialog::message::Kind::Error,
            ),
            _ => dialog.notice(
                "导入失败：返回结果类型不正确",
                crate::native_dialog::message::Kind::Error,
            ),
        }
    }
}
