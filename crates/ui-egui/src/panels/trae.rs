use super::*;
use crate::tabs::trae::{Dialog, State};
use tomato_services::trae::{self, Action, Mutation};
#[derive(Clone)]
struct Drag {
    phone: String,
    path: PathBuf,
}

pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let view = tool.trae.get_or_insert_with(Default::default);
    let path = view.path.clone().unwrap_or_else(trae::legacy_path);
    let command = |action| {
        Intent::command(Command::Trae {
            path: path.clone(),
            action,
        })
    };
    let Some(snapshot) = view.snapshot.clone() else {
        ui.heading("Trae 会话管理");
        if !busy && (view.status.is_empty() || ui.button("重新加载").clicked()) {
            view.status = "正在读取会话配置".into();
            return Some(command(Action::Load));
        }
        return None;
    };
    let now = ui.input(|i| i.time);
    if now < view.status_until {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut mutation = None;
    ui.spacing_mut().item_spacing = egui::vec2(10., 10.);
    ui.spacing_mut().interact_size.y = 22.;
    egui::Frame::new()
        .fill(Color32::from_rgb(247, 248, 250))
        .inner_margin(egui::Margin::symmetric(10, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18., 18.), egui::Sense::hover());
                icons.paint(ui, "images/tools/trae-header.svg", rect);
                ui.label(RichText::new("Trae 会话管理").size(16.).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("管理 Trae CN 多账号会话，一键切换 User 目录")
                            .size(12.)
                            .color(Color32::from_gray(153)),
                    );
                });
            });
        });
    if now < view.status_until && !view.status.is_empty() {
        egui::Panel::bottom(ui.id().with("trae-status"))
            .exact_size(34.)
            .resizable(false)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.label(&view.status);
            });
    }
    let modal = view.dialog.is_some();
    egui::Panel::left(ui.id().with("trae-sessions"))
        .default_size(280.)
        .min_size(150.)
        .max_size((ui.available_width() * 0.7).max(150.))
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            egui::Frame::new().inner_margin(10).show(ui, |ui| {
                ui.label(RichText::new("会话列表").size(14.).strong());
                if ui
                    .add_enabled(
                        !busy && !modal && snapshot.user_exists,
                        egui::Button::new(
                            RichText::new("+ 添加当前账号")
                                .size(12.)
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(33, 150, 243))
                        .min_size(egui::vec2(ui.available_width(), 28.)),
                    )
                    .clicked()
                {
                    let active = snapshot.config.sessions.iter().find(|s| s.active);
                    view.dialog = Some(Dialog::Add {
                        phone: active.map(|s| s.phone.clone()).unwrap_or_default(),
                        name: active.map(|s| s.name.clone()).unwrap_or_default(),
                    });
                }
                if !snapshot.user_exists {
                    ui.label(
                        RichText::new("User 目录不存在，请先登录 Trae CN 账号")
                            .size(11.)
                            .color(Color32::from_gray(153)),
                    );
                }
            });
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, 57., snapshot.config.sessions.len(), |ui, rows| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    for index in rows {
                        let s = &snapshot.config.sessions[index];
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 57.),
                            egui::Sense::click_and_drag(),
                        );
                        let selected = view.selected.as_deref() == Some(&s.phone);
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0.,
                                if selected {
                                    Color32::from_rgb(232, 244, 255)
                                } else {
                                    Color32::from_gray(245)
                                },
                            );
                        }
                        let avatar = egui::Rect::from_min_size(
                            rect.min + egui::vec2(10., 8.),
                            egui::vec2(40., 40.),
                        );
                        ui.painter()
                            .rect_filled(avatar, 8., Color32::from_rgb(232, 244, 255));
                        icons.paint(ui, "images/tools/trae-person.svg", avatar.shrink(9.));
                        let text_rect = egui::Rect::from_min_max(
                            rect.min + egui::vec2(62., 9.),
                            egui::pos2(rect.right() - 54., rect.bottom() - 8.),
                        );
                        let painter = ui.painter().with_clip_rect(text_rect);
                        painter.text(
                            text_rect.min,
                            egui::Align2::LEFT_TOP,
                            &s.name,
                            egui::FontId::proportional(14.),
                            Color32::from_gray(51),
                        );
                        painter.text(
                            text_rect.min + egui::vec2(0., 22.),
                            egui::Align2::LEFT_TOP,
                            &s.phone,
                            egui::FontId::proportional(11.),
                            Color32::from_gray(153),
                        );
                        let toggle = egui::Rect::from_min_size(
                            egui::pos2(rect.right() - 48., rect.center().y - 10.),
                            egui::vec2(38., 20.),
                        );
                        let toggled = ui.place(toggle, |ui: &mut Ui| {
                            ui.add_enabled_ui(!busy && !modal, |ui| {
                                super::hosts::switch(ui, s.active)
                            })
                            .inner
                        });
                        if toggled.clicked() {
                            view.pending_selection = Some(s.phone.clone());
                            mutation = Some(if s.active {
                                Mutation::Close {
                                    phone: s.phone.clone(),
                                }
                            } else {
                                Mutation::Activate {
                                    phone: s.phone.clone(),
                                }
                            });
                        }
                        if response.clicked() && !modal {
                            view.select(s.phone.clone());
                        }
                        response.context_menu(|ui| {
                            ui.add_enabled_ui(!busy && !modal, |ui| {
                                if ui.button("切换到此会话").clicked() {
                                    view.pending_selection = Some(s.phone.clone());
                                    mutation = Some(Mutation::Activate {
                                        phone: s.phone.clone(),
                                    });
                                    ui.close();
                                }
                                if ui.button("编辑名称").clicked() {
                                    view.dialog = Some(Dialog::Rename {
                                        phone: s.phone.clone(),
                                        name: s.name.clone(),
                                    });
                                    ui.close();
                                }
                                if ui.button("在资源管理器中打开").clicked() {
                                    open(view, &snapshot.config, &s.phone, now);
                                    ui.close();
                                }
                                if ui
                                    .button(
                                        RichText::new("删除会话")
                                            .color(Color32::from_rgb(229, 57, 53)),
                                    )
                                    .clicked()
                                {
                                    view.dialog = Some(Dialog::Delete {
                                        phone: s.phone.clone(),
                                        active: s.active,
                                        step: 0,
                                    });
                                    ui.close();
                                }
                            });
                        });
                        if !busy && !modal {
                            if response.drag_started() {
                                response.dnd_set_drag_payload(Drag {
                                    phone: s.phone.clone(),
                                    path: path.clone(),
                                });
                            }
                            if let Some(payload) = response
                                .dnd_hover_payload::<Drag>()
                                .filter(|p| p.path == path && p.phone != s.phone)
                            {
                                let after = ui.input(|i| {
                                    i.pointer
                                        .interact_pos()
                                        .is_some_and(|p| p.y > rect.center().y)
                                });
                                let y = if after { rect.bottom() } else { rect.top() };
                                ui.painter().hline(
                                    rect.x_range(),
                                    y,
                                    egui::Stroke::new(2., Color32::from_rgb(33, 150, 243)),
                                );
                                if response.dnd_release_payload::<Drag>().is_some() {
                                    mutation =
                                        view.reorder(&payload.phone, Some((&s.phone, after)));
                                }
                            }
                        }
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom(),
                            egui::Stroke::new(1., Color32::from_gray(240)),
                        );
                    }
                });
            let response = ui.interact(
                ui.available_rect_before_wrap(),
                ui.id().with("trae-drop-end"),
                egui::Sense::hover(),
            );
            if !busy
                && !modal
                && let Some(payload) = response
                    .dnd_release_payload::<Drag>()
                    .filter(|p| p.path == path)
            {
                mutation = view.reorder(&payload.phone, None);
            }
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::new().inner_margin(10))
        .show(ui, |ui| {
            let selected = view
                .selected
                .as_ref()
                .and_then(|p| snapshot.config.sessions.iter().position(|s| &s.phone == p));
            let Some(index) = selected else {
                ui.label(RichText::new("请选择一个会话").size(16.).strong());
                return;
            };
            let s = &snapshot.config.sessions[index];
            ui.label(RichText::new(&s.name).size(16.).strong());
            ui.label(format!("手机号: {}", s.phone));
            ui.label(
                RichText::new(if s.active {
                    "状态: 当前活动会话"
                } else {
                    "状态: 未激活"
                })
                .strong()
                .color(if s.active {
                    Color32::from_rgb(76, 175, 80)
                } else {
                    Color32::from_gray(153)
                }),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !busy && !modal && !s.active,
                        egui::Button::new(RichText::new("切换到此会话").color(Color32::WHITE))
                            .fill(Color32::from_rgb(76, 175, 80)),
                    )
                    .clicked()
                {
                    mutation = Some(Mutation::Activate {
                        phone: s.phone.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        !modal,
                        egui::Button::new(
                            RichText::new("在资源管理器中打开").color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(255, 152, 0)),
                    )
                    .clicked()
                {
                    open(view, &snapshot.config, &s.phone, now);
                }
            });
            ui.separator();
            if let Ok(dir) = trae::directory(&snapshot.config, &s.phone) {
                ui.label(
                    RichText::new(format!(
                        "目录: {}",
                        dir.to_string_lossy().replace('\\', "/")
                    ))
                    .size(12.)
                    .color(Color32::from_gray(119)),
                );
            }
            ui.label(
                RichText::new(if snapshot.exists.get(index) == Some(&true) {
                    "● 目录存在"
                } else {
                    "● 目录不存在"
                })
                .size(12.)
                .color(if snapshot.exists.get(index) == Some(&true) {
                    Color32::from_rgb(76, 175, 80)
                } else {
                    Color32::from_rgb(229, 57, 53)
                }),
            );
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("历史任务").size(14.).strong());
                if !view.tasks.is_empty() {
                    ui.label(
                        RichText::new(format!("(共 {} 条)", view.tasks.len()))
                            .size(12.)
                            .color(Color32::from_gray(153)),
                    );
                }
            });
            if !view.task_status.is_empty() {
                ui.label(&view.task_status);
            }
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    let width = ui.available_width();
                    if (view.task_width - width).abs() > 1. {
                        view.task_heights.fill(54.);
                        view.task_width = width;
                    }
                    let origin = ui.cursor().top();
                    // Cache bounded previews; full text is retained only once for double-click copy.
                    for (index, text) in view.task_labels.iter().enumerate() {
                        let top = ui.cursor().top() - origin;
                        let height = view.task_heights[index];
                        if top + height < viewport.top() - 100. || top > viewport.bottom() + 100. {
                            ui.allocate_space(egui::vec2(width, height));
                            continue;
                        }
                        let response = egui::Frame::new()
                            .fill(Color32::from_gray(249))
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_min_width((width - 16.).max(0.));
                                ui.label(
                                    RichText::new(format!("#{}", index + 1))
                                        .size(11.)
                                        .strong()
                                        .color(Color32::from_rgb(25, 118, 210)),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(text).size(12.))
                                        .wrap()
                                        .sense(egui::Sense::click()),
                                );
                            })
                            .response
                            .interact(egui::Sense::click());
                        view.task_heights[index] = response.rect.height();
                        if response.double_clicked() {
                            ui.ctx().copy_text(view.tasks[index].clone());
                            view.status = "已复制到剪贴板".into();
                            view.status_until = now + 4.;
                        }
                    }
                });
        });
    if let Some(dialog) = &mut view.dialog {
        let mut accept = false;
        let mut cancel = false;
        let mut keep = false;
        egui::Window::new(match dialog {
            Dialog::Add { .. } => "添加账号",
            Dialog::Rename { .. } => "编辑名称",
            Dialog::Delete { step: 0, .. } => "删除确认",
            Dialog::Delete { .. } => "删除会话",
        })
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            match dialog {
                Dialog::Add { phone, name } => {
                    ui.label("将当前 User 目录重命名为 User{手机号}，登记为非活动会话");
                    ui.label("手机号:");
                    ui.text_edit_singleline(phone);
                    ui.label("名称:");
                    ui.text_edit_singleline(name);
                    if let Some(parent) = snapshot.config.user_dir.parent() {
                        ui.label(format!(
                            "归档目录: {}",
                            parent.join(format!("User{phone}")).display()
                        ));
                    }
                }
                Dialog::Rename { name, .. } => {
                    ui.label("修改会话名称");
                    ui.text_edit_singleline(name);
                }
                Dialog::Delete { phone, step, .. } => {
                    ui.label(if *step == 0 {
                        format!("确定要删除会话 “{phone}” 吗？")
                    } else {
                        format!("是否同时删除该会话的数据目录？ User{phone}")
                    });
                }
            }
            ui.horizontal(|ui| {
                accept = ui.add_enabled(!busy, egui::Button::new("确定")).clicked();
                if matches!(dialog, Dialog::Delete { step: 1, .. }) {
                    keep = ui
                        .add_enabled(!busy, egui::Button::new("保留数据目录"))
                        .clicked();
                }
                cancel = ui.button("取消").clicked();
            });
        });
        if accept || keep {
            match dialog {
                Dialog::Add { phone, name } => {
                    view.pending_selection = Some(phone.trim().into());
                    mutation = Some(Mutation::Archive {
                        phone: phone.trim().into(),
                        name: name.trim().into(),
                    });
                }
                Dialog::Rename { phone, name } => {
                    mutation = Some(Mutation::Rename {
                        phone: phone.clone(),
                        name: name.trim().into(),
                    })
                }
                Dialog::Delete {
                    phone,
                    active,
                    step,
                } => {
                    if *active || *step == 1 {
                        mutation = Some(Mutation::Delete {
                            phone: phone.clone(),
                            delete_data: !*active && !keep,
                        });
                    } else {
                        *step = 1;
                    }
                }
            }
        }
        if cancel {
            view.dialog = None;
        }
    }
    if !busy {
        if let Some(mutation) = mutation {
            return Some(command(Action::Mutate {
                previous: Box::new((*snapshot).clone()),
                mutation,
            }));
        }
        if view.pending_tasks
            && let Some(phone) = view.selected.clone()
        {
            view.pending_tasks = false;
            return Some(command(Action::Tasks {
                config: snapshot.config.clone(),
                phone,
                generation: view.generation,
            }));
        }
    }
    None
}
fn open(view: &mut State, config: &trae::Config, phone: &str, now: f64) {
    match trae::directory(config, phone).and_then(|p| {
        anyhow::ensure!(p.is_dir(), "目录不存在: {}", p.display());
        open::that(p).map_err(Into::into)
    }) {
        Ok(()) => {}
        Err(error) => {
            view.status = format!("无法打开目录: {error:#}");
            view.status_until = now + 4.;
        }
    }
}
