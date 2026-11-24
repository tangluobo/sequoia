//! Controller for the original Java connection export window.
use super::*;
use tomato_core::config::export::{Credentials, Selection};
#[cfg(test)]
mod tests;
mod view;

pub(super) struct Dialog {
    selection: Selection,
    collapsed: std::collections::BTreeSet<String>,
    include_passwords: bool,
    positioned: bool,
    focused: Option<String>,
    focus_tree: bool,
    phase: Phase,
    notice_focus: bool,
}
enum Phase {
    Ready,
    Writing {
        id: u64,
        control: tomato_services::backup::Control,
    },
    Warning(String),
    Error(String),
    Done(String),
}
impl Default for Dialog {
    fn default() -> Self {
        Self {
            selection: Selection::default(),
            collapsed: Default::default(),
            include_passwords: false,
            positioned: false,
            focused: None,
            focus_tree: true,
            phase: Phase::Ready,
            notice_focus: true,
        }
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        if let Phase::Writing { control, .. } = &self.phase {
            control.cancel();
        }
    }
}
impl TomatoApp {
    pub(super) fn preview_connection_export(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要独立空配置"))?;
        anyhow::ensure!(
            profile.is_absolute()
                && profile
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("tomato-export-fixture-"))
                && self.store.connections.is_empty(),
            "需要独立空配置"
        );
        self.store.connections = tomato_core::legacy::decode(
            r#"[{"id":"group","name":"项目目录"},{"id":"ssh","name":"Linux 测试服务器","type":"SSH","parentId":"group","host":"127.0.0.1","port":9},{"id":"db","name":"MySQL 测试数据库","type":"MYSQL","parentId":"group","host":"127.0.0.1","port":9},{"id":"nested","name":"个人目录"},{"id":"s3","name":"S3 对象存储","type":"S3","parentId":"nested"}]"#,
        )?;
        self.export();
        if std::env::var("TOMATO_EXPORT_SELECTION").as_deref() == Ok("partial") {
            let dialog = self.export_dialog.as_mut().unwrap();
            let tree = dialog.selection.tree(&self.store.connections);
            dialog.selection.set(&tree, 1, true);
        }
        Ok(())
    }
    pub(super) fn connection_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        match view::render(ctx, dialog, &self.store.connections, &mut self.icons) {
            view::Action::None => {}
            view::Action::Cancel => self.export_dialog = None,
            view::Action::Apply => {
                if dialog
                    .selection
                    .tree(&self.store.connections)
                    .rows
                    .iter()
                    .all(|r| r.check == tomato_core::config::export::Check::Unchecked)
                {
                    dialog.phase = Phase::Warning("请至少选择一个连接".into());
                    return;
                }
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("导出连接")
                    .add_filter("JSON 文件", &["json"])
                    .set_file_name("connections_export.json")
                    .save_file()
                {
                    self.export_connections_to(path);
                }
            }
        }
    }
    pub(super) fn export_connections_to(&mut self, path: PathBuf) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        if !matches!(dialog.phase, Phase::Ready) {
            return;
        }
        let credentials = if dialog.include_passwords {
            Credentials::Included
        } else {
            Credentials::Redacted
        };
        let request = dialog
            .selection
            .prepare(&self.store.connections, credentials);
        let control = tomato_services::backup::Control::default();
        let result = request.and_then(|prepared| {
            self.queue.submit(Command::ConnectionExport {
                source: self.repository.location().map(PathBuf::from),
                path,
                prepared,
                control: Some(control.clone()),
            })
        });
        match result {
            Ok(id) => {
                dialog.phase = Phase::Writing { id, control };
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::ConnectionExport,
                    },
                );
            }
            Err(error) => dialog.phase = Phase::Error(format!("导出失败：{error:#}")),
        }
    }
    pub(super) fn connection_export_reply(&mut self, id: u64, result: anyhow::Result<Output>) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        if !matches!(&dialog.phase, Phase::Writing { id: active, .. } if *active == id) {
            return;
        }
        match result {
            Ok(Output::Text(message)) => {
                self.status = message.clone();
                dialog.phase = Phase::Done(message);
            }
            Ok(_) => dialog.phase = Phase::Error("导出失败：返回结果类型不正确".into()),
            Err(error) => dialog.phase = Phase::Error(format!("导出失败：{error:#}")),
        }
    }
}
