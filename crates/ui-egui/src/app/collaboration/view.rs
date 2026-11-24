use super::*;
use cloud::{Command, Detail, Kind, ProjectAction, Role};
use egui::{ComboBox, TextWrapMode};

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    values: impl IntoIterator<Item = (T, String)>,
    label: String,
) {
    ComboBox::from_id_salt(id)
        .width(180.)
        .selected_text(label)
        .wrap_mode(TextWrapMode::Extend)
        .show_ui(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
            ui.spacing_mut().interact_size.y = 24.;
            for (candidate, label) in values {
                ui.selectable_value(value, candidate, label);
            }
        });
}
pub(super) fn show(
    ui: &mut Ui,
    state: &mut State,
    icons: &mut crate::icons::Icons,
) -> Option<Action> {
    ui.spacing_mut().item_spacing = egui::vec2(8., 7.);
    ui.spacing_mut().interact_size.y = 26.;
    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
    let mut action = None;
    let snapshot = state.view.clone();
    if state.view.user.is_none() {
        if !snapshot.accounts.is_empty() {
            let mut index = snapshot
                .accounts
                .iter()
                .position(|a| a.base == state.base && a.email == state.email)
                .unwrap_or(usize::MAX);
            let old = index;
            let label = snapshot
                .accounts
                .get(index)
                .map(|a| format!("{} · {}", a.email, a.base))
                .unwrap_or_else(|| "已保存的服务器账号".into());
            combo(
                ui,
                "cloud-accounts",
                &mut index,
                snapshot
                    .accounts
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (i, format!("{} · {}", a.email, a.base))),
                label,
            );
            if index != old
                && let Some(a) = snapshot.accounts.get(index)
            {
                state.base = a.base.clone();
                state.email = a.email.clone();
            }
        }
        ui.label("同步连接设置与工作文件，密码保留在本机。");
        egui::Grid::new("cloud-login")
            .min_col_width(95.)
            .spacing([10., 9.])
            .show(ui, |ui| {
                ui.label("服务器地址");
                ui.add(TextEdit::singleline(&mut state.base).desired_width(460.));
                ui.end_row();
                ui.label("邮箱");
                ui.add(TextEdit::singleline(&mut state.email).desired_width(300.));
                ui.end_row();
                ui.label("密码");
                ui.add(
                    TextEdit::singleline(&mut state.password)
                        .password(true)
                        .desired_width(300.),
                );
                ui.end_row();
                ui.label("验证码／恢复码");
                ui.add(TextEdit::singleline(&mut state.code).desired_width(300.));
                ui.end_row();
                ui.label("显示名称");
                ui.add(TextEdit::singleline(&mut state.name).desired_width(300.));
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!state.view.busy, egui::Button::new("登录"))
                .clicked()
            {
                action = Some(Action::Send(Command::Connect {
                    base: state.base.clone(),
                    email: state.email.clone(),
                    password: std::mem::take(&mut state.password),
                    code: std::mem::take(&mut state.code),
                }));
            }
            if ui.button("注册").clicked() {
                action = Some(Action::Send(Command::Register {
                    base: state.base.clone(),
                    email: state.email.clone(),
                    name: state.name.clone(),
                    password: std::mem::take(&mut state.password),
                }));
            }
            if ui.button("打开离线副本").clicked() {
                action = Some(Action::Send(Command::Offline {
                    base: state.base.clone(),
                    email: state.email.clone(),
                }));
            }
            if ui.button("网页管理").clicked() {
                action = Some(Action::Portal);
            }
            if ui.button("清理离线副本").clicked() {
                action = Some(Action::Send(Command::ClearCache));
            }
        });
        ui.separator();
    }
    if let Some(user) = &state.view.user {
        ui.horizontal_wrapped(|ui| {
            ui.strong(&user.name);
            ui.label(&user.email);
            ui.label(if state.view.online {
                "已连接"
            } else {
                "离线 · 待恢复"
            });
            if ui.button("同步").clicked() {
                action = Some(Action::Send(Command::Refresh));
            }
            if ui.button("邀请").clicked() {
                state.detail = Some("邀请");
                action = Some(Action::Send(Command::Detail(Detail::Invitations)));
            }
            if ui.button("账号与安全").clicked() {
                action = Some(Action::Portal);
            }
            if ui.button("同步个人配置").clicked() {
                action = Some(Action::PublishPersonal);
            }
            if ui.button("个人配置／冲突").clicked() {
                action = Some(Action::Send(Command::Load("@personal".into())));
            }
            if ui.button("退出登录").clicked() {
                action = Some(Action::Send(Command::Disconnect));
            }
            let mut push = snapshot.push;
            if ui.checkbox(&mut push, "推送同步").changed() {
                action = Some(Action::Send(Command::Push(push)));
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label("项目");
        ui.add(
            TextEdit::singleline(&mut state.project_name)
                .hint_text("项目名称")
                .desired_width(180.),
        );
        if ui.button("新建项目").clicked() {
            action = Some(Action::Send(Command::Project(ProjectAction::Create(
                state.project_name.clone(),
            ))));
        }
        if let Some(p) = state
            .view
            .projects
            .iter()
            .find(|p| p.id == state.view.selected)
        {
            if p.role.manage() && ui.button("重命名").clicked() {
                action = Some(Action::Send(Command::Project(ProjectAction::Rename(
                    p.clone(),
                    state.project_name.clone(),
                ))));
            }
            if p.role == Role::Owner {
                if ui.button("删除空项目").clicked() {
                    action = Some(Action::Send(Command::Project(ProjectAction::Delete(
                        p.id.clone(),
                    ))));
                }
            } else if ui.button("退出项目").clicked() {
                action = Some(Action::Send(Command::Project(ProjectAction::Leave(
                    p.id.clone(),
                ))));
            }
        }
    });
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(180., 420.),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.strong("所有项目");
                ScrollArea::vertical()
                    .id_salt("cloud-projects")
                    .max_height(420.)
                    .show(ui, |ui| {
                        for p in &snapshot.projects {
                            let response = ui.add_sized(
                                [172., 28.],
                                egui::Button::selectable(p.id == state.view.selected, &p.name)
                                    .wrap_mode(TextWrapMode::Truncate),
                            );
                            if response.clicked() {
                                if state.draft.as_ref().is_some_and(|d| d.dirty()) {
                                    state.error = Some("请先保存或撤回当前草稿".into());
                                } else {
                                    state.reset_draft();
                                    state.selected.clear();
                                    state.detail = None;
                                    state.project_name = p.name.clone();
                                    action = Some(Action::Send(Command::Select(p.id.clone())));
                                }
                            }
                            response.on_hover_text(p.role.label());
                        }
                    });
            },
        );
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width().min(450.));
            let project = state
                .view
                .projects
                .iter()
                .find(|p| p.id == state.view.selected)
                .cloned();
            let write = state
                .draft
                .as_ref()
                .is_some_and(|d| d.project == "@personal")
                || project.as_ref().is_some_and(|p| p.role.write());
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(write, |ui| {
                    ui.menu_button("新建对象", |ui| {
                        ui.set_min_width(160.);
                        for kind in Kind::ALL.into_iter().filter(|kind| *kind != Kind::Profile) {
                            if ui.button(kind.label()).clicked() {
                                action = Some(Action::New(kind));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("上传工作文件", |ui| {
                        for kind in [
                            Kind::Model,
                            Kind::Bi,
                            Kind::Query,
                            Kind::Snippet,
                            Kind::Pipeline,
                        ] {
                            if ui.button(kind.label()).clicked() {
                                action = Some(Action::Import(kind));
                                ui.close();
                            }
                        }
                    });
                    if ui.button("共享选中连接副本").clicked() {
                        action = Some(Action::PublishConnection);
                    }
                    if ui.button("共享当前查询副本").clicked() {
                        action = Some(Action::PublishQuery);
                    }
                });
                if let Some(p) = &project {
                    if ui.button("成员").clicked() {
                        state.detail = Some("成员");
                        action = Some(Action::Send(Command::Detail(Detail::Members(p.id.clone()))));
                    }
                    if ui.button("活动").clicked() {
                        state.detail = Some("活动");
                        action = Some(Action::Send(Command::Detail(Detail::Activity(
                            p.id.clone(),
                        ))));
                    }
                }
                ui.add(
                    TextEdit::singleline(&mut state.filter)
                        .hint_text("搜索对象")
                        .desired_width(160.),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("目标项目");
                let mut index = state
                    .view
                    .projects
                    .iter()
                    .position(|p| p.id == state.target)
                    .unwrap_or(usize::MAX);
                let label = state
                    .view
                    .projects
                    .get(index)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "请选择".into());
                combo(
                    ui,
                    "cloud-target",
                    &mut index,
                    state
                        .view
                        .projects
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.id != state.view.selected && p.role.write())
                        .map(|(i, p)| (i, p.name.clone())),
                    label,
                );
                if let Some(p) = state.view.projects.get(index) {
                    state.target = p.id.clone();
                }
                if ui.button("复制选中").clicked() {
                    action = Some(Action::Transfer(true));
                }
                if ui
                    .add_enabled(write, egui::Button::new("移动选中"))
                    .clicked()
                {
                    action = Some(Action::Transfer(false));
                }
            });
            ui.horizontal(|ui| {
                ui.label("选择");
                ui.label(RichText::new("名称 / 类型 / 状态").strong());
            });
            let visible = state
                .view
                .items
                .iter()
                .filter(|o| {
                    o.object
                        .name
                        .to_lowercase()
                        .contains(&state.filter.to_lowercase())
                })
                .collect::<Vec<_>>();
            ScrollArea::vertical()
                .id_salt("cloud-objects")
                .max_height(170.)
                .show_rows(ui, 26., visible.len(), |ui, range| {
                    for index in range {
                        let cached = visible[index];
                        let object = &cached.object;
                        ui.horizontal(|ui| {
                            let mut checked = state.selected.contains(&object.id);
                            if ui.checkbox(&mut checked, "").changed() {
                                if checked {
                                    state.selected.insert(object.id.clone());
                                } else {
                                    state.selected.remove(&object.id);
                                }
                            }
                            let suffix = if cached.error.is_some() {
                                "上传受阻"
                            } else if cached.conflict {
                                "⚠ 冲突"
                            } else if cached.dirty {
                                "待上传"
                            } else if object.deleted {
                                "已删除"
                            } else {
                                "已同步"
                            };
                            let (rect, row) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 26.),
                                egui::Sense::click(),
                            );
                            if state
                                .view
                                .document
                                .as_ref()
                                .is_some_and(|d| d.id == object.id)
                            {
                                ui.painter()
                                    .rect_filled(rect, 2, ui.visuals().selection.bg_fill);
                            } else if row.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    2,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            let icon = match object.kind {
                                Kind::Group => "images/connect/folder.png",
                                Kind::Query | Kind::Snippet => "images/connect/query.png",
                                Kind::Connection => crate::icons::connection(
                                    state
                                        .view
                                        .bindings
                                        .iter()
                                        .find(|(_, o)| o.id == object.id)
                                        .and_then(|(_, o)| o.body["type"].as_str())
                                        .unwrap_or("MYSQL"),
                                ),
                                _ => "images/connect/table.png",
                            };
                            icons.paint(
                                ui,
                                icon,
                                egui::Rect::from_center_size(
                                    egui::pos2(rect.left() + 12., rect.center().y),
                                    egui::vec2(16., 16.),
                                ),
                            );
                            let text =
                                egui::Rect::from_min_max(rect.min + egui::vec2(26., 0.), rect.max);
                            let mut job = egui::text::LayoutJob::simple_singleline(
                                format!(
                                    "{}    {}    v{}    {}",
                                    object.name,
                                    object.kind.label(),
                                    object.revision,
                                    suffix
                                ),
                                egui::TextStyle::Body.resolve(ui.style()),
                                ui.visuals().text_color(),
                            );
                            job.wrap.max_width = text.width();
                            job.wrap.max_rows = 1;
                            job.wrap.overflow_character = Some('…');
                            let galley = ui.painter().layout_job(job);
                            let position =
                                egui::pos2(text.left(), text.center().y - galley.size().y / 2.);
                            ui.painter()
                                .with_clip_rect(text.intersect(ui.clip_rect()))
                                .galley(position, galley, ui.visuals().text_color());
                            if let Some(error) = &cached.error {
                                row.clone().on_hover_text(error);
                            }
                            if row.clicked() {
                                if state.draft.as_ref().is_some_and(|d| d.dirty()) {
                                    state.error = Some("请先保存或撤回当前草稿".into());
                                } else {
                                    action = Some(Action::Edit(object.id.clone()));
                                }
                            }
                            row.context_menu(|ui| {
                                ui.set_min_width(180.);
                                ui.set_max_width(220.);
                                if ui.button("打开／编辑").clicked() {
                                    action = Some(Action::Edit(object.id.clone()));
                                    ui.close();
                                }
                                if ui.button("版本历史").clicked() {
                                    state.detail = Some("版本历史");
                                    action = Some(Action::Send(Command::Detail(Detail::History(
                                        object.project_id.clone(),
                                        object.id.clone(),
                                    ))));
                                    ui.close();
                                }
                                if ui.button("下载").clicked() {
                                    action = Some(Action::Export(object.id.clone()));
                                    ui.close();
                                }
                                if ui.add_enabled(write, egui::Button::new("删除")).clicked() {
                                    action = Some(Action::Delete(object.id.clone()));
                                    ui.close();
                                }
                            });
                        });
                    }
                });
            ui.separator();
            if let Some(draft) = &mut state.draft {
                ui.horizontal(|ui| {
                    ui.label(draft.kind.label());
                    ui.add_enabled(
                        write,
                        TextEdit::singleline(&mut draft.name).desired_width(200.),
                    );
                    if ui.add_enabled(write, egui::Button::new("保存")).clicked() {
                        action = Some(Action::Save);
                    }
                    if ui.button("撤回草稿").clicked() {
                        action = Some(Action::Edit(draft.id.clone()));
                    }
                    if ui.button("下载").clicked() {
                        action = Some(Action::Export(draft.id.clone()));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("所属连接／分组");
                    let mut parent = draft.parent.clone();
                    let label = state
                        .view
                        .items
                        .iter()
                        .find(|o| o.object.id == parent)
                        .map(|o| o.object.name.clone())
                        .unwrap_or_else(|| "项目根目录".into());
                    ComboBox::from_id_salt("cloud-parent")
                        .width(240.)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            ui.set_min_width(240.);
                            ui.selectable_value(&mut parent, String::new(), "项目根目录");
                            for o in &state.view.items {
                                if matches!(o.object.kind, Kind::Connection | Kind::Group)
                                    && o.object.id != draft.id
                                    && !o.object.deleted
                                {
                                    ui.selectable_value(
                                        &mut parent,
                                        o.object.id.clone(),
                                        &o.object.name,
                                    );
                                }
                            }
                        });
                    if write {
                        draft.parent = parent;
                    }
                });
                ScrollArea::vertical()
                    .id_salt("cloud-document")
                    .max_height(210.)
                    .show(ui, |ui| {
                        if draft
                            .source
                            .as_ref()
                            .is_some_and(|o| o.body["base64"].is_string())
                        {
                            let source = draft.source.as_ref().unwrap();
                            ui.label(format!(
                                "工作文件：{} · {} KiB",
                                source.body["filename"].as_str().unwrap_or(&draft.name),
                                source.bytes.div_ceil(1024)
                            ));
                            ui.label("工作区文件按原始内容保存，下载后可用对应编辑器打开。");
                        } else if draft.kind == Kind::Connection {
                            ui.add_enabled_ui(write, |ui| connection_editor(ui, draft));
                        } else {
                            ui.add_enabled(
                                write,
                                TextEdit::multiline(&mut draft.text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(7)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    if matches!(draft.kind, Kind::Connection | Kind::Group)
                        && ui.button("接入连接树").clicked()
                    {
                        action = Some(Action::Bind);
                    }
                    if ui
                        .add_enabled(write, egui::Button::new("更新工作文件"))
                        .clicked()
                    {
                        action = Some(Action::ReplaceFile);
                    }
                    if matches!(draft.kind, Kind::Query | Kind::Snippet)
                        && ui.button("在查询编辑器打开").clicked()
                    {
                        action = Some(Action::OpenQuery);
                    }
                    if ui.button("版本历史").clicked() {
                        state.detail = Some("版本历史");
                        action = Some(Action::Send(Command::Detail(Detail::History(
                            draft.project.clone(),
                            draft.id.clone(),
                        ))));
                    }
                    if ui
                        .add_enabled(
                            write,
                            egui::Button::new(if draft.revision == 0 {
                                "删除草稿"
                            } else {
                                "删除对象"
                            }),
                        )
                        .clicked()
                    {
                        action = Some(Action::Delete(draft.id.clone()));
                    }
                });
                if let Some(remote) = &state.view.remote {
                    ui.colored_label(
                        Color32::from_rgb(180, 85, 0),
                        format!("检测到远端 v{}，两份内容都已保留。", remote.revision),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("采用本地版本").clicked() {
                            action = Some(Action::Send(Command::Resolve {
                                id: draft.id.clone(),
                                keep_local: true,
                            }));
                        }
                        if ui.button("采用远端版本").clicked() {
                            action = Some(Action::Send(Command::Resolve {
                                id: draft.id.clone(),
                                keep_local: false,
                            }));
                        }
                    });
                    egui::CollapsingHeader::new("查看远端内容").show(ui, |ui| {
                        let mut text =
                            serde_json::to_string_pretty(&remote.body).unwrap_or_default();
                        ui.add(
                            TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false)
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
            }
        });
    });
    if let Some(detail) = state.detail {
        details(ui, state, detail, &mut action);
    }
    ui.separator();
    ui.horizontal(|ui| {
        if state.view.busy {
            ui.spinner();
        }
        ui.add(egui::Label::new(&state.view.message).wrap_mode(TextWrapMode::Wrap));
    });
    action
}
fn connection_editor(ui: &mut Ui, draft: &mut Draft) {
    let Ok(mut body) = serde_json::from_str::<serde_json::Value>(&draft.text) else {
        ui.add(TextEdit::multiline(&mut draft.text).desired_width(f32::INFINITY));
        return;
    };
    let mut changed = false;
    egui::Grid::new("cloud-connection-fields")
        .spacing([10., 7.])
        .show(ui, |ui| {
            for (key, label) in [
                ("type", "类型"),
                ("host", "主机"),
                ("username", "用户名"),
                ("database", "数据库"),
                ("schema", "Schema"),
            ] {
                ui.label(label);
                let mut value = body[key].as_str().unwrap_or_default().to_owned();
                if ui
                    .add_sized(
                        [270., 24.],
                        TextEdit::singleline(&mut value).desired_width(270.),
                    )
                    .changed()
                {
                    body[key] = value.into();
                    changed = true;
                }
                ui.end_row();
            }
            ui.label("端口");
            let mut port = body["port"].as_u64().unwrap_or(3306);
            if ui
                .add(egui::DragValue::new(&mut port).range(1..=65535))
                .changed()
            {
                body["port"] = port.into();
                changed = true;
            }
            ui.end_row();
            ui.label("SSL / TLS");
            let mut tls = body["databaseTls"].as_bool().unwrap_or(false);
            if ui.checkbox(&mut tls, "启用数据库 TLS").changed() {
                body["databaseTls"] = tls.into();
                changed = true;
            }
            ui.end_row();
            ui.label("SSH 隧道");
            let mut tunnel = body["useSshTunnel"].as_bool().unwrap_or(false);
            if ui.checkbox(&mut tunnel, "通过 SSH 连接").changed() {
                body["useSshTunnel"] = tunnel.into();
                changed = true;
            }
            ui.end_row();
            if tunnel {
                for (key, label) in [
                    ("sshTunnelHost", "跳板主机"),
                    ("sshTunnelUsername", "SSH 用户"),
                ] {
                    ui.label(label);
                    let mut value = body[key].as_str().unwrap_or_default().to_owned();
                    if ui
                        .add_sized(
                            [270., 24.],
                            TextEdit::singleline(&mut value).desired_width(270.),
                        )
                        .changed()
                    {
                        body[key] = value.into();
                        changed = true;
                    }
                    ui.end_row();
                }
                ui.label("SSH 端口");
                let mut port = body["sshTunnelPort"].as_u64().unwrap_or(22);
                if ui
                    .add(egui::DragValue::new(&mut port).range(1..=65535))
                    .changed()
                {
                    body["sshTunnelPort"] = port.into();
                    changed = true;
                }
                ui.end_row();
            }
        });
    if changed {
        draft.text = serde_json::to_string_pretty(&body).unwrap_or_default();
    }
    ui.small("密码与密钥在接入后的本地连接设置中填写。");
    egui::CollapsingHeader::new("高级设置 JSON").show(ui, |ui| {
        ui.add(
            TextEdit::multiline(&mut draft.text)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    });
}
fn details(ui: &mut Ui, state: &mut State, title: &str, action: &mut Option<Action>) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("cloud-details")
                .max_height(180.)
                .show(ui, |ui| {
                    if title == "成员" {
                        for member in state.view.detail.as_array().into_iter().flatten() {
                            ui.horizontal(|ui| {
                                ui.label(member["email"].as_str().unwrap_or_default());
                                let current: Role = serde_json::from_value(member["role"].clone())
                                    .unwrap_or_default();
                                let mut role = current;
                                combo(
                                    ui,
                                    member["id"].as_str().unwrap_or("member"),
                                    &mut role,
                                    [Role::Manager, Role::Editor, Role::Viewer]
                                        .map(|r| (r, r.label().into())),
                                    current.label().into(),
                                );
                                if current != Role::Owner && role != current {
                                    *action = Some(Action::Send(Command::Project(
                                        ProjectAction::Member {
                                            project: state.view.selected.clone(),
                                            member: member["id"]
                                                .as_str()
                                                .unwrap_or_default()
                                                .into(),
                                            role: Some(role),
                                        },
                                    )));
                                }
                                if current != Role::Owner && ui.button("移除").clicked() {
                                    *action = Some(Action::Send(Command::Project(
                                        ProjectAction::Member {
                                            project: state.view.selected.clone(),
                                            member: member["id"]
                                                .as_str()
                                                .unwrap_or_default()
                                                .into(),
                                            role: None,
                                        },
                                    )));
                                }
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut state.invite_email)
                                    .hint_text("邀请邮箱")
                                    .desired_width(200.),
                            );
                            let label = state.invite_role.label().into();
                            combo(
                                ui,
                                "cloud-invite-role",
                                &mut state.invite_role,
                                [Role::Manager, Role::Editor, Role::Viewer]
                                    .map(|r| (r, r.label().into())),
                                label,
                            );
                            if ui.button("邀请成员").clicked() {
                                *action =
                                    Some(Action::Send(Command::Project(ProjectAction::Invite {
                                        project: state.view.selected.clone(),
                                        email: state.invite_email.clone(),
                                        role: state.invite_role,
                                    })));
                            }
                        });
                    } else if title == "邀请" {
                        for invitation in state.view.detail.as_array().into_iter().flatten() {
                            ui.horizontal(|ui| {
                                ui.label(invitation["project"].as_str().unwrap_or_default());
                                ui.label(invitation["email"].as_str().unwrap_or_default());
                                for (accept, label) in [(true, "接受"), (false, "拒绝")] {
                                    if ui.button(label).clicked() {
                                        *action = Some(Action::Send(Command::Project(
                                            ProjectAction::Invitation {
                                                id: invitation["id"]
                                                    .as_str()
                                                    .unwrap_or_default()
                                                    .into(),
                                                accept,
                                            },
                                        )));
                                    }
                                }
                            });
                        }
                    } else if title == "版本历史" {
                        if let Some(entries) = state.view.detail.as_array() {
                            for h in entries {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "v{} · {} · {} bytes",
                                        h["revision"],
                                        h["name"].as_str().unwrap_or_default(),
                                        h["bytes"]
                                    ));
                                    if ui.button("预览").clicked()
                                        && let Some(d) = &state.draft
                                    {
                                        *action =
                                            Some(Action::Send(Command::Detail(Detail::Version {
                                                project: d.project.clone(),
                                                id: d.id.clone(),
                                                revision: h["revision"].as_i64().unwrap_or(0),
                                            })));
                                    }
                                });
                            }
                        } else if let Ok(old) =
                            serde_json::from_value::<cloud::Object>(state.view.detail.clone())
                        {
                            ui.label(format!("历史版本 v{}", old.revision));
                            let mut text =
                                serde_json::to_string_pretty(&old.body).unwrap_or_default();
                            ui.add(
                                TextEdit::multiline(&mut text)
                                    .interactive(false)
                                    .desired_width(f32::INFINITY),
                            );
                            if ui.button("恢复为新版本").clicked()
                                && let Some(current) = &state.view.document
                            {
                                let mut mutation = cloud::Mutation::from_object(&old);
                                mutation.base_revision = current.revision;
                                mutation.deleted = false;
                                *action = Some(Action::Send(Command::Save {
                                    project: current.project_id.clone(),
                                    id: current.id.clone(),
                                    mutation,
                                }));
                            }
                        }
                    } else {
                        let mut text =
                            serde_json::to_string_pretty(&state.view.detail).unwrap_or_default();
                        ui.add(
                            TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false)
                                .desired_width(f32::INFINITY),
                        );
                    }
                });
        });
}
