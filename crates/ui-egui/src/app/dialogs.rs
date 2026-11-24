use super::*;
mod music;
mod storage;
mod tunnel;

impl TomatoApp {
    pub(super) fn connection_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let folder = dialog.draft.kind.is_none();
        let music = music::is_music(&dialog.draft);
        let title = if folder {
            "新建目录"
        } else if music {
            "无损音乐下载配置"
        } else if dialog.editing {
            "编辑连接"
        } else {
            "新建连接"
        };
        let mut open = true;
        let mut cancel = false;

        let mut apply = false;
        let mut back = false;
        egui::Window::new(title)
            .id(egui::Id::new(if music {
                "music-config"
            } else {
                "connection-config"
            }))
            .open(&mut open)
            .default_size(if folder {
                [300.0, 160.0]
            } else if music {
                [490.0, 210.0]
            } else {
                [780.0, 800.0]
            })
            .min_size(if folder {
                [300.0, 160.0]
            } else if music {
                [400.0, 170.0]
            } else {
                [560.0, 520.0]
            })
            .show(ctx, |ui| {
                egui::Frame::new().inner_margin(20).show(ui, |ui| {
                    if !music {
                        ui.label(
                            RichText::new(if folder { "新建目录" } else { title })
                                .size(16.0)
                                .strong(),
                        );
                        ui.add_space(15.0);
                    }
                    if folder {
                        ui.label("目录名称：");
                        ui.add(
                            TextEdit::singleline(&mut dialog.draft.name)
                                .hint_text("请输入目录名称"),
                        );
                    } else {
                        let kind = dialog.draft.kind().to_string();
                        let has_tunnel = matches!(
                            kind.as_str(),
                            "MYSQL"
                                | "POSTGRESQL"
                                | "ORACLE"
                                | "HIVE"
                                | "REDIS"
                                | "MONGODB"
                                | "SSH"
                                | "SFTP"
                                | "KAFKA"
                                | "ROCKETMQ"
                                | "S3"
                                | "ALIYUN_OSS"
                        );
                        if has_tunnel {
                            ui.horizontal(|ui| {
                                for (index, label) in ["常规", "SSH通道"].iter().enumerate() {
                                    ui.selectable_value(&mut dialog.tab, index, *label);
                                }
                            });
                            ui.separator();
                        }
                        ScrollArea::vertical().max_height(570.0).show(ui, |ui| {
                            if dialog.tab == 1 && has_tunnel {
                                tunnel::fields(ui, &mut dialog.draft, &self.store.connections);
                            } else {
                                egui::Grid::new("general-config")
                                    .spacing([10.0, 10.0])
                                    .show(ui, |ui| {
                                        ui.label("名称：");
                                        ui.add_sized(
                                            [if kind == "ADB" { 320. } else { 280. }, 28.0],
                                            TextEdit::singleline(&mut dialog.draft.name)
                                                .vertical_align(if kind == "ADB" {
                                                    egui::Align::Center
                                                } else {
                                                    egui::Align::Min
                                                })
                                                .hint_text(if kind == "ADB" {
                                                    "连接名称"
                                                } else {
                                                    ""
                                                }),
                                        );
                                        ui.end_row();
                                        if kind == "ADB" {
                                            adb_config::fields(
                                                ui,
                                                &mut dialog.draft,
                                                &mut dialog.adb_port,
                                            );
                                        } else if kind == "LOCAL_DIRECTORY" {
                                            storage::directory(ui, &mut dialog.draft);
                                        } else if matches!(
                                            kind.as_str(),
                                            "ALIYUN" | "TENCENT_CLOUD"
                                        ) {
                                            form_field(
                                                ui,
                                                &mut dialog.draft,
                                                "username",
                                                if kind == "ALIYUN" {
                                                    "AccessKey ID："
                                                } else {
                                                    "SecretId："
                                                },
                                                false,
                                            );
                                            form_field(
                                                ui,
                                                &mut dialog.draft,
                                                "password",
                                                if kind == "ALIYUN" {
                                                    "AccessKey Secret："
                                                } else {
                                                    "SecretKey："
                                                },
                                                true,
                                            );
                                            check(
                                                ui,
                                                &mut dialog.draft,
                                                "savePassword",
                                                "保存密钥",
                                            );
                                        } else if matches!(kind.as_str(), "S3" | "ALIYUN_OSS") {
                                            storage::fields(ui, &mut dialog.draft, false);
                                        } else if kind == "HTTP_SERVER"
                                            || kind == "FTP_SERVER"
                                            || kind == "SMB_SERVER"
                                        {
                                            form_field(
                                                ui,
                                                &mut dialog.draft,
                                                "localDirectoryPath",
                                                "目录：",
                                                false,
                                            );
                                        } else if kind == "LOCAL_TERMINAL" {
                                            ui.label("终端类型：");
                                            let mut shell =
                                                dialog.draft.text("terminalType").to_string();
                                            egui::ComboBox::from_id_salt("terminal-type")
                                                .selected_text(if shell.is_empty() {
                                                    "PowerShell"
                                                } else {
                                                    &shell
                                                })
                                                .show_ui(ui, |ui| {
                                                    for value in ["powershell", "cmd", "system"] {
                                                        ui.selectable_value(
                                                            &mut shell,
                                                            value.into(),
                                                            value,
                                                        );
                                                    }
                                                });
                                            dialog.draft.set("terminalType", shell);
                                            ui.end_row();
                                        } else if kind == "TOOL" {
                                            if music {
                                                music::fields(ui, &mut dialog.draft);
                                            }
                                        } else {
                                            for (key, label, secret) in [
                                                ("host", "主机：", false),
                                                ("port", "端口：", false),
                                                ("username", "用户名：", false),
                                                ("password", "密码：", true),
                                            ] {
                                                form_field(
                                                    ui,
                                                    &mut dialog.draft,
                                                    key,
                                                    label,
                                                    secret,
                                                );
                                            }
                                            if matches!(kind.as_str(), "MONGODB" | "SQLITE") {
                                                form_field(
                                                    ui,
                                                    &mut dialog.draft,
                                                    "database",
                                                    "默认/认证数据库：",
                                                    false,
                                                );
                                            }
                                            if matches!(kind.as_str(), "REDIS" | "MONGODB") {
                                                ui.label("");
                                                check(
                                                    ui,
                                                    &mut dialog.draft,
                                                    "savePassword",
                                                    "保存密码",
                                                );
                                                ui.end_row();
                                            }
                                            if kind == "REDIS" {
                                                tunnel::redis(ui, &mut dialog.draft);
                                            }
                                            if kind == "RDP" {
                                                for (key, label) in [
                                                    ("domain", "域："),
                                                    ("screenWidth", "分辨率宽度："),
                                                    ("screenHeight", "分辨率高度："),
                                                    ("colorDepth", "颜色深度："),
                                                ] {
                                                    form_field(
                                                        ui,
                                                        &mut dialog.draft,
                                                        key,
                                                        label,
                                                        false,
                                                    );
                                                }
                                            }
                                        }
                                        if kind != "ADB" && !music {
                                            form_field(
                                                ui,
                                                &mut dialog.draft,
                                                "description",
                                                "备注：",
                                                false,
                                            );
                                        }
                                    });
                                if kind == "ADB" {
                                    ui.add_space(12.);
                                    dialog.adb_pairing.show(
                                        ui,
                                        self.runtime.as_ref().map(|runtime| runtime.handle()),
                                    );
                                }
                                if kind == "SSH" || kind == "SFTP" {
                                    check(ui, &mut dialog.draft, "usePassword", "使用密码认证");
                                    check(ui, &mut dialog.draft, "useKey", "使用密钥认证");
                                    if dialog.draft.flag("useKey") {
                                        let mut paths = dialog
                                            .draft
                                            .options
                                            .get("privateKeyPaths")
                                            .and_then(serde_json::Value::as_array)
                                            .map(|paths| {
                                                paths
                                                    .iter()
                                                    .filter_map(serde_json::Value::as_str)
                                                    .map(str::to_owned)
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default();
                                        let mut remove = None;
                                        for (index, path) in paths.iter().enumerate() {
                                            ui.horizontal(|ui| {
                                                ui.label(path);
                                                if ui.small_button("移除").clicked() {
                                                    remove = Some(index);
                                                }
                                            });
                                        }
                                        if let Some(index) = remove {
                                            paths.remove(index);
                                        }
                                        if ui.button("+ 添加密钥").clicked()
                                            && let Some(path) = rfd::FileDialog::new()
                                                .set_title("选择 SSH 私钥")
                                                .pick_file()
                                        {
                                            paths.push(path.to_string_lossy().into_owned());
                                        }
                                        dialog
                                            .draft
                                            .set("privateKeyPaths", serde_json::json!(paths));
                                    }
                                }
                                if kind == "RDP" {
                                    for (key, label) in [
                                        ("fullscreen", "全屏"),
                                        ("useSsl", "使用SSL/TLS加密"),
                                        ("mapClipboard", "剪贴板同步"),
                                        ("enableSound", "启用声音"),
                                    ] {
                                        check(ui, &mut dialog.draft, key, label);
                                    }
                                }
                            }
                        });
                    }
                    ui.add_space(15.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.load_failed
                                    && !dialog.draft.name.trim().is_empty()
                                    && (!music
                                        || !music::directory(&dialog.draft).trim().is_empty()),
                                crate::theme::green_button("确定").min_size(egui::vec2(80.0, 28.0)),
                            )
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .add_sized([80.0, 28.0], egui::Button::new("取消"))
                            .clicked()
                        {
                            cancel = true;
                        }
                        if !dialog.editing
                            && !folder
                            && ui
                                .add_sized([80.0, 28.0], egui::Button::new("返回"))
                                .clicked()
                        {
                            back = true;
                        }
                    });
                });
            });
        if apply {
            self.apply_connection_draft();
        } else if !open || cancel || back {
            self.dialog = None;
            if back {
                self.picker = Some(if music { 5 } else { 0 });
            }
        }
    }
    pub(super) fn apply_connection_draft(&mut self) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let mut draft = dialog.draft.clone();
        let music_update = if music::is_music(&draft) {
            let directory = music::directory(&draft).trim().to_owned();
            if directory.is_empty() {
                self.error = Some("请选择下载目录".into());
                return;
            }
            draft.set(crate::panels::music::DOWNLOAD_DIRECTORY, directory.clone());
            Some((draft.id.clone(), draft.name.clone(), directory))
        } else {
            None
        };
        if let Err(error) = adb_config::normalize(&mut draft, &dialog.adb_port) {
            self.error = Some(error.to_string());
            return;
        }
        tunnel::normalize(&mut draft);
        if draft.options.get("savePassword") == Some(&serde_json::Value::Bool(false)) {
            draft.set("password", serde_json::Value::Null);
        }
        let mut next = self.store.clone();
        if let Some(index) = next.connections.iter().position(|c| c.id == draft.id) {
            next.connections[index] = draft;
        } else {
            next.connections.push(draft);
        }
        match tomato_core::config::validate(&next.connections) {
            Ok(()) => {
                if self.persist_store(next) {
                    if let Some((id, name, directory)) = music_update {
                        for tab in self.tabs.iter_mut().filter(|tab| tab.source_id == id) {
                            if let Panel::Tool(tool) = &mut tab.panel
                                && let Some(state) = &mut tool.music
                            {
                                state.set_directory(directory.clone());
                                tab.title.clone_from(&name);
                            }
                        }
                    }
                    self.dialog = None;
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(super) fn confirm_dialog(&mut self, ctx: &egui::Context) {
        if self.confirmation.is_none() {
            return;
        }
        let mut accepted = false;
        let mut cancel = false;
        let mut save = false;
        let mut keep_children = false;
        let folder = match &self.confirmation {
            Some(Confirmation::Delete(ids)) if ids.len() == 1 => {
                self.store.connections.iter().find(|c| {
                    c.id == ids[0]
                        && c.kind.is_none()
                        && self
                            .store
                            .connections
                            .iter()
                            .any(|child| child.parent_id.as_deref() == Some(&c.id))
                })
            }
            _ => None,
        };
        let editor = match &self.confirmation {
            Some(Confirmation::Close(id)) => {
                self.tabs
                    .iter()
                    .find(|tab| tab.id == *id)
                    .and_then(|tab| match &tab.panel {
                        Panel::Editor { .. } => Some((id.clone(), tab.title.clone(), tab.busy)),
                        Panel::Sql { workbench, .. } if workbench.document.is_some() => Some((
                            id.clone(),
                            tab.title.clone(),
                            workbench
                                .document
                                .as_ref()
                                .is_some_and(|document| document.loading || document.saving),
                        )),
                        _ => None,
                    })
            }
            _ => None,
        };
        egui::Window::new("确认操作")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if let Some((_, title, _)) = &editor {
                    ui.label(format!("文件 \"{title}\" 已修改未保存，是否保存？"));
                } else {
                    if let Some(Confirmation::Delete(ids)) = &self.confirmation {
                        if let Some(folder) = folder {
                            ui.label(format!("确定要删除目录 \"{}\" 吗？", folder.name));
                            ui.label("该目录下包含子节点，请选择操作：");
                        } else if ids.len() == 1 {
                            if let Some(c) = self.store.connections.iter().find(|c| c.id == ids[0])
                            {
                                ui.label(format!("确定要删除 \"{}\" 吗？", c.name));
                            }
                        } else {
                            ui.label(format!("确定要删除选中的 {} 个节点吗？", ids.len()));
                            ui.label("所选目录会连同其子节点一起删除，此操作无法撤销。");
                        }
                    } else {
                        ui.label("有尚未保存的文件，放弃更改？");
                    }
                }
                ui.horizontal(|ui| {
                    if folder.is_some() {
                        keep_children = ui.button("保留子节点").clicked();
                    }
                    if let Some((_, _, busy)) = &editor {
                        save = ui.add_enabled(!*busy, egui::Button::new("保存")).clicked();
                    }
                    accepted = ui
                        .button(if editor.is_some() {
                            "不保存"
                        } else if folder.is_some() {
                            "连同子节点一起删除"
                        } else {
                            "确认"
                        })
                        .clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
        if save {
            if let Some((id, _, _)) = &editor
                && let Some(Tab {
                    panel: Panel::Sql { workbench, .. },
                    ..
                }) = self.tabs.iter_mut().find(|tab| &tab.id == id)
                && let Some(document) = &mut workbench.document
            {
                document.close_after_save = true;
                let id = id.clone();
                self.confirmation = None;
                self.query_action(query_tree::Action::Save(id));
                return;
            }
            if let Some((id, _, _)) = editor
                && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                && let Panel::Editor {
                    path,
                    text,
                    markdown,
                    ..
                } = &mut tab.panel
            {
                markdown.close_after_save = true;
                let intent = Intent::Save(path.clone(), text.clone());
                self.confirmation = None;
                self.handle_intent(&id, intent);
            }
        } else if accepted || keep_children {
            match self.confirmation.take() {
                Some(Confirmation::Delete(ids)) => {
                    if !self.delete_connections(&ids, keep_children) {
                        self.confirmation = Some(Confirmation::Delete(ids));
                    }
                }
                Some(Confirmation::Close(id)) => self.close(&id),
                Some(Confirmation::Quit) => {
                    self.quit_approved = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        } else if cancel {
            if let Some(owner) = self.file_owner_close.take() {
                self.closing_tabs.retain(|id| id != &owner);
            }
            self.confirmation = None;
        }
    }
}

fn form_field(ui: &mut Ui, connection: &mut Connection, key: &str, label: &str, secret: bool) {
    ui.label(label);
    let numeric = matches!(
        key,
        "port" | "sshTunnelPort" | "screenWidth" | "screenHeight" | "colorDepth" | "redisDatabase"
    );
    let mut value = if numeric {
        connection
            .options
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(match key {
                "screenWidth" => 1024,
                "screenHeight" => 768,
                "colorDepth" => 24,
                "sshTunnelPort" => 22,
                _ => 0,
            })
            .to_string()
    } else {
        connection.text(key).to_string()
    };
    if ui
        .add_sized(
            [if numeric { 100.0 } else { 280.0 }, 28.0],
            TextEdit::singleline(&mut value).password(secret),
        )
        .changed()
    {
        if numeric {
            if let Ok(value) = value.parse::<u64>() {
                connection.set(key, value);
            }
        } else {
            connection.set(key, value);
        }
    }
    ui.end_row();
}
fn check(ui: &mut Ui, connection: &mut Connection, key: &str, label: &str) {
    let mut value = connection
        .options
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(matches!(
            key,
            "usePassword"
                | "useSsl"
                | "mapClipboard"
                | "enableSound"
                | "savePassword"
                | "sshTunnelUsePassword"
        ));
    if ui.checkbox(&mut value, label).changed() {
        connection.set(key, value);
    }
}
