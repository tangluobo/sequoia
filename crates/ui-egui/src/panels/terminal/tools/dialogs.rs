mod form;
mod storage;
use super::files::join;
use super::*;
pub(super) use storage::{action as storage_action, is_storage};
pub(crate) fn render(ui: &mut Ui, workspace: &mut Workspace) {
    if let Some(editor) = &mut workspace.editor {
        let mut open = true;
        let mut save = false;
        egui::Window::new(format!("编辑 - {}", editor.path))
            .id(ui.make_persistent_id("sftp-editor"))
            .default_size([750., 500.])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!editor.pending, egui::Button::new("保存"))
                        .clicked()
                    {
                        save = true;
                    }
                    ui.label(if editor.text == editor.saved {
                        "已保存"
                    } else {
                        "未保存"
                    });
                });
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24),
                    );
                });
            });
        if save {
            editor.pending = true;
            let op = Operation::WriteText {
                path: editor.path.clone(),
                text: editor.text.clone(),
            };
            workspace.enqueue(Section::Files, op);
        }
        if !open {
            if workspace.editor.as_ref().is_some_and(|e| e.text != e.saved) {
                workspace.panes.entry(Section::Files).or_default().status =
                    "文件有未保存修改，请保存后关闭".into();
            } else {
                workspace.editor = None;
            }
        }
    }
    if let Some(text) = &workspace.output {
        let mut open = true;
        egui::Window::new("执行结果")
            .id(ui.make_persistent_id("ssh-output"))
            .default_size([800., 500.])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
                });
            });
        if !open {
            workspace.output = None;
        }
    }
    let mut accept = false;
    let mut cancel = false;
    let blocked =
        workspace.busy(Section::Storage) && workspace.dialog.as_ref().is_some_and(is_storage);
    if let Some(dialog) = &mut workspace.dialog {
        (accept, cancel) = form::show(ui, dialog, blocked);
    }
    if cancel {
        workspace.dialog = None;
    }
    if accept && let Some(mut dialog) = workspace.dialog.take() {
        if matches!(dialog.kind, Form::Notice) {
            return;
        }
        let values = &dialog.fields;
        let value = |i: usize| values.get(i).cloned().unwrap_or_default();
        if let Some(operation) = dialog.confirm.take() {
            let section = match &operation {
                Operation::Manage(ManagementAction::Docker { .. }) => Section::Docker,
                Operation::Manage(
                    ManagementAction::StopProcesses(_) | ManagementAction::AllowPort { .. },
                ) => Section::Ports,
                Operation::Manage(ManagementAction::Service { .. }) => Section::Services,
                Operation::Manage(_) => Section::Storage,
                _ => Section::Files,
            };
            workspace.enqueue(section, operation);
        } else {
            let operation = if let Some(action) = storage_action(&dialog) {
                match action {
                    Ok(action) => Some(Operation::Manage(action)),
                    Err(error) => {
                        workspace.storage.fail(error.to_string());
                        workspace.dialog = Some(dialog);
                        return;
                    }
                }
            } else {
                match dialog.kind {
                    Form::Mkdir | Form::Create => {
                        let path = workspace
                            .panes
                            .get(&Section::Files)
                            .map_or(".", |pane| pane.path.as_str());
                        if tomato_services::terminal::ssh::zmodem::safe_name(&value(0)) {
                            Some(if matches!(dialog.kind, Form::Mkdir) {
                                Operation::Mkdir(join(path, &value(0)))
                            } else {
                                Operation::Create(join(path, &value(0)))
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            if let Some(operation) = operation {
                if matches!(operation, Operation::Manage(_)) {
                    confirm(workspace, storage::confirmation(&dialog.kind), operation);
                } else {
                    workspace.enqueue(Section::Files, operation);
                }
            } else {
                workspace.panes.entry(Section::Files).or_default().status = "名称无效".into();
            }
        }
    }
}

pub(crate) fn confirm(workspace: &mut Workspace, title: &str, operation: Operation) {
    workspace.dialog = Some(Dialog {
        focus: true,
        title: title.into(),
        labels: vec![],
        fields: vec![],
        kind: Form::Confirm,
        confirm: Some(operation),
    });
}

pub(crate) fn storage_dialog(kind: Form, path: String) -> Dialog {
    let (title, labels, fields) = match &kind {
        Form::InitializePv => (
            "新硬盘初始化为 PV",
            vec!["整盘设备（例如 /dev/sdb）"],
            vec![path],
        ),
        Form::VolumeGroup => (
            "创建/扩展 VG",
            vec!["PV 设备", "VG 名称"],
            vec![path, "vg_data".into()],
        ),
        Form::ExtendLv => (
            "扩展 LV",
            vec!["LV 路径", "增加容量"],
            vec![path, "+10G".into()],
        ),
        _ => (
            "格式化/挂载",
            vec!["设备", "挂载点", "文件系统", "写入 /etc/fstab"],
            vec![path, "/data".into(), "不格式化".into(), "true".into()],
        ),
    };
    Dialog {
        focus: true,
        title: title.into(),
        labels: labels.into_iter().map(str::to_string).collect(),
        fields,
        kind,
        confirm: None,
    }
}

pub(super) fn notice(workspace: &mut Workspace, message: &str) {
    workspace.panes.entry(Section::Files).or_default().status = message.into();
    workspace.dialog = Some(Dialog {
        focus: true,
        title: "提示".into(),
        labels: Vec::new(),
        fields: vec![message.into()],
        kind: Form::Notice,
        confirm: None,
    });
}
