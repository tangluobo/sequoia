use super::*;
pub(super) fn perform(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    action: Option<String>,
    navigate: Option<String>,
) -> Option<Intent> {
    let mut intent = None;
    let section = Section::Files;
    let object = tomato_services::files::object_copy::is_object(connection);
    let mode = workspace.file_view.clone();
    let pane = workspace.panes.entry(section).or_default();
    let path = pane.path.clone();
    let bucket_root = bucket_icon(connection, &path).is_some();
    let selected: Vec<_> = if action.is_none() {
        Vec::new()
    } else if mode == "COLUMN" && pane.selected.is_empty() {
        pane.column_selected
            .as_deref()
            .and_then(|p| pane.file_entry(p))
            .cloned()
            .into_iter()
            .collect()
    } else {
        pane.selected
            .iter()
            .filter_map(|i| pane.entries.get(*i).cloned())
            .collect()
    };
    if let Some(path) = navigate {
        workspace.enqueue(section, Operation::List(path));
    }
    if let Some(action) = action {
        if bucket_root
            && matches!(
                action.as_str(),
                "新建目录" | "创建文件" | "上传文件..." | "上传目录..." | "粘贴" | "粘贴上传"
            )
        {
            let message = match action.as_str() {
                "新建目录" => "请先进入一个 Bucket 再创建目录",
                "创建文件" => "请先进入一个 Bucket 再创建文件",
                _ => "请先进入一个 Bucket 再上传或粘贴",
            };
            super::dialogs::notice(workspace, message);
            return intent;
        }
        match action.as_str() {
            "ICON" | "LIST" | "COLUMN" => {
                workspace.file_view = action;
                workspace.document_active = None;
            }
            "编辑 Markdown" => {
                if let Some(entry) = selected.first() {
                    intent = Some(Intent::OpenFileDocument(entry.path.clone()));
                }
            }
            "复制" if object => {
                if !selected.is_empty() {
                    workspace.enqueue(
                        section,
                        Operation::CopyObjects {
                            directory: if mode == "COLUMN" && selected.len() == 1 {
                                parent(&selected[0].path)
                            } else {
                                path.clone()
                            },
                            paths: selected
                                .iter()
                                .map(|e| (e.path.clone(), e.directory))
                                .collect(),
                        },
                    );
                }
            }
            "粘贴" | "粘贴上传" if object => workspace.paste_objects(ui.ctx(), path.clone()),
            "复制访问地址" => {
                if let Some(entry) = selected.first().filter(|e| !e.directory) {
                    if let Some(url) = access_url(connection, &entry.path) {
                        ui.ctx().copy_text(url);
                        workspace.panes.entry(section).or_default().status =
                            "已复制访问地址".into();
                    } else {
                        workspace.panes.entry(section).or_default().status =
                            "请先在连接设置中配置访问URL".into();
                    }
                }
            }
            "粘贴上传" => workspace.enqueue(section, Operation::ClipboardFiles),
            "预览图片" => {
                if let Some(entry) = selected.first() {
                    workspace.open_image(&entry.path);
                }
            }
            "打开" => {
                if let Some(entry) = selected.first() {
                    if !entry.directory && connection.kind() != "FTP" && menu::markdown(&entry.name)
                    {
                        intent = Some(Intent::OpenFileDocument(entry.path.clone()));
                    } else if !entry.directory
                        && crate::tabs::ssh::images::supported_connection(connection)
                        && tomato_services::files::images::supported(&entry.name)
                    {
                        workspace.open_image(&entry.path);
                    } else {
                        workspace.enqueue(
                            section,
                            if entry.directory {
                                Operation::List(entry.path.clone())
                            } else {
                                Operation::PrepareEdit {
                                    remote: entry.path.clone(),
                                    launch: true,
                                }
                            },
                        );
                    }
                }
            }
            "刷新" => {
                if matches!(
                    workspace.file_status,
                    tomato_services::terminal::Status::Failed(_)
                        | tomato_services::terminal::Status::Exited
                ) {
                    workspace.reconnect_requested = true;
                } else {
                    let pane = workspace.panes.entry(section).or_default();
                    pane.thumbnails.suspend();
                    pane.refresh = true;
                }
            }
            "上传文件..." | "上传目录..." => {
                let files = if action == "上传目录..." {
                    rfd::FileDialog::new()
                        .set_title("选择上传目录")
                        .pick_folder()
                        .map(|p| vec![p])
                } else {
                    rfd::FileDialog::new()
                        .set_title("选择上传文件")
                        .pick_files()
                };
                if let Some(files) = files {
                    workspace.upload_files(ui.ctx(), connection, path.clone(), files, Vec::new());
                }
            }
            "下载..." => {
                if selected.len() == 1 && !selected[0].directory {
                    let entry = &selected[0];
                    if let Some(local) = rfd::FileDialog::new()
                        .set_file_name(&entry.name)
                        .save_file()
                    {
                        workspace.enqueue(
                            section,
                            Operation::Download {
                                remote: entry.path.clone(),
                                local,
                            },
                        );
                    }
                } else if !selected.is_empty()
                    && let Some(directory) = rfd::FileDialog::new()
                        .set_title("下载到目录（同名文件将覆盖）")
                        .pick_folder()
                {
                    let operations = selected
                        .into_iter()
                        .filter(|e| tomato_services::terminal::ssh::zmodem::safe_name(&e.name))
                        .map(|e| Operation::Download {
                            remote: e.path,
                            local: directory.join(e.name),
                        })
                        .collect();
                    confirm(
                        workspace,
                        "下载所选项到目录（同名文件将覆盖）",
                        Operation::Batch(operations),
                    );
                }
            }
            "新建目录" | "创建文件" | "新建Bucket" => {
                workspace.dialog = Some(Dialog {
                    focus: true,
                    title: action.clone(),
                    labels: vec!["名称".into()],
                    fields: vec![if action == "创建文件" {
                        "新文件.md".into()
                    } else {
                        String::new()
                    }],
                    kind: if action != "创建文件" {
                        Form::Mkdir
                    } else {
                        Form::Create
                    },
                    confirm: None,
                })
            }
            "重命名" => {
                if !bucket_root
                    && selected.len() == 1
                    && bucket_icon(connection, &parent(&selected[0].path)).is_none()
                {
                    let pane = workspace.panes.entry(section).or_default();
                    pane.finish_file_rename(ui.ctx(), false);
                    pane.rename = Some(crate::tabs::ssh::RenameDraft::new(
                        &selected[0],
                        (&pane.path, &pane.filter, &mode),
                        None,
                    ));
                    ui.ctx().request_repaint();
                }
            }
            "删除" if !selected.is_empty() => {
                if bucket_root && connection.kind() == "ALIYUN_OSS" {
                    workspace.panes.entry(section).or_default().status =
                        "删除失败: 请通过管理控制台删除Bucket".into();
                    return intent;
                }
                let title = if selected.len() == 1 {
                    format!("确定要删除 \"{}\" 吗？", selected[0].name)
                } else {
                    format!("确定要删除选中的 {} 个文件/目录吗？", selected.len())
                };
                let operations = selected
                    .into_iter()
                    .map(|entry| Operation::Remove {
                        path: entry.path,
                        directory: entry.directory,
                    })
                    .collect();
                confirm(workspace, &title, Operation::Batch(operations));
            }
            _ => {}
        }
    }
    intent
}
