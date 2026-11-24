use super::*;
pub(super) fn edit_selected(view: &mut ServerView, account: bool) {
    let Some(index) = view.selected else {
        return;
    };
    view.dialog = if account {
        view.config
            .accounts
            .get(index)
            .cloned()
            .map(|value| Edit::Account {
                index: Some(index),
                value,
            })
    } else {
        view.config
            .shared_directories
            .get(index)
            .cloned()
            .map(|value| Edit::Share {
                index: Some(index),
                users: value.allowed_users.join(", "),
                value,
            })
    };
}
pub(super) fn render(
    ui: &mut Ui,
    view: &mut ServerView,
    error: &mut Option<String>,
    changed: &mut bool,
) -> Option<Intent> {
    let mut accept = false;
    let mut cancel = false;
    let mut intent = None;
    if let Some(edit) = &mut view.dialog {
        let title = match edit {
            Edit::Share { index, .. } => {
                if index.is_some() {
                    "编辑共享目录"
                } else {
                    "添加共享目录"
                }
            }
            Edit::Account { index, .. } => {
                if index.is_some() {
                    "编辑账号"
                } else {
                    "添加账号"
                }
            }
            Edit::Remove { .. } => "确认删除",
            Edit::ConfirmStart => "启动服务",
        };
        egui::Window::new(title)
            .id(egui::Id::new("server-config-dialog"))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::new().inner_margin(18).show(ui, |ui| {
                    ui.set_min_width(404.);
                    ui.spacing_mut().interact_size.y = 14.;
                    ui.spacing_mut().button_padding = egui::vec2(10., 4.);
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
                    match edit {
                        Edit::Share { value, users, .. } => super::forms::share(ui, value, users),
                        Edit::Account { value, .. } => super::forms::account(ui, value),
                        Edit::Remove { indices, account } => {
                            ui.label(format!("确定删除选中的 {} 个{}？", indices.len(), if *account { "账号" } else { "共享目录" }));
                        }
                        Edit::ConfirmStart => {
                            ui.label("未配置共享目录、根目录或账号主目录。\nHTTP将显示欢迎页，FTP默认使用用户主目录。继续？");
                        }
                    }
                    ui.add_space(14.);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_sized([80., 27.], egui::Button::new(RichText::new("确定").color(Color32::WHITE)).fill(Color32::from_rgb(7,193,96)).corner_radius(4)).clicked() { accept = true; }
                        if ui.add_sized([80., 27.], egui::Button::new("取消")).clicked() { cancel = true; }
                    });
                });
            });
    }
    if accept && let Some(edit) = view.dialog.take() {
        match edit {
            Edit::Share {
                index,
                mut value,
                users,
            } => {
                value.alias = value.alias.trim().to_owned();
                value.path = value.path.trim().to_owned();
                value.allowed_users = users
                    .split([',', '，'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                let mut candidate = view.config.clone();
                if let Some(index) = index {
                    candidate.shared_directories[index] = value.clone();
                } else {
                    candidate.shared_directories.push(value.clone());
                }
                if let Err(reason) = candidate.validate() {
                    *error = Some(reason.to_string());
                    view.dialog = Some(Edit::Share {
                        index,
                        value,
                        users,
                    });
                } else {
                    view.log(format!(
                        "{}共享目录：{}",
                        if index.is_some() { "修改" } else { "添加" },
                        value.alias
                    ));
                    view.config = candidate;
                    *changed = true;
                }
            }
            Edit::Account { index, mut value } => {
                value.username = value.username.trim().to_owned();
                value.home_directory = value.home_directory.trim().to_owned();
                let mut candidate = view.config.clone();
                if let Some(index) = index {
                    candidate.accounts[index] = value.clone();
                } else {
                    candidate.accounts.push(value.clone());
                }
                if let Err(reason) = candidate.validate() {
                    *error = Some(reason.to_string());
                    view.dialog = Some(Edit::Account { index, value });
                } else {
                    view.log(format!(
                        "{}账号：{}",
                        if index.is_some() { "修改" } else { "添加" },
                        value.username
                    ));
                    view.config = candidate;
                    *changed = true;
                }
            }
            Edit::Remove { indices, account } => {
                // Descending indices keep the remaining selected rows stable.
                for index in indices.into_iter().rev() {
                    if account && index < view.config.accounts.len() {
                        let removed = view.config.accounts.remove(index);
                        view.log(format!("删除账号：{}", removed.username));
                    } else if !account && index < view.config.shared_directories.len() {
                        let removed = view.config.shared_directories.remove(index);
                        view.log(format!("删除共享目录：{}", removed.alias));
                    }
                }
                view.clear_selection();
                *changed = true;
            }
            Edit::ConfirmStart => {
                intent = Some(Intent::StartConfigured(view.config.clone()));
            }
        }
    }
    if cancel {
        view.dialog = None;
    }
    intent
}
