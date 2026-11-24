use super::*;
use crate::tabs::redis::{Failure, State};

pub(super) fn render(ui: &mut Ui, view: &mut State, busy: bool, action: &mut Option<Action>) {
    let mut validation = None;
    let mut close = false;
    if let Some(dialog) = &mut view.dialog {
        let title = match dialog {
            Dialog::Add { .. } => "添加Key",
            Dialog::Rename { .. } => "重命名Key",
            Dialog::Ttl { .. } => "设置TTL",
            Dialog::Delete { .. } => "删除Key",
        };
        let focus = std::mem::take(&mut view.dialog_focus);
        let mut open = true;
        egui::Window::new(title)
            .id(ui.id().with("redis-operation"))
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(360.)
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().button_padding = egui::vec2(10., 3.);
                ui.spacing_mut().interact_size.y = 23.;
                ui.add_enabled_ui(!busy && view.failure.is_none(), |ui| {
                    match dialog {
                        Dialog::Add { key, kind, text } => {
                            ui.label("Key:");
                            let id = view.dialog_editors[0].prepare(ui, "redis-dialog-main");
                            let response =
                                ui.add(TextEdit::singleline(key).id(id).desired_width(340.));
                            if focus {
                                response.request_focus();
                            }
                            ui.label("类型:");
                            egui::ComboBox::from_id_salt("redis-new-type")
                                .width(340.)
                                .selected_text(kind.to_uppercase())
                                .show_ui(ui, |ui| {
                                    for value in ["string", "list", "set", "zset", "hash"] {
                                        ui.selectable_value(
                                            kind,
                                            value.into(),
                                            value.to_uppercase(),
                                        );
                                    }
                                });
                            ui.label("值:");
                            let id = view.dialog_editors[1].prepare(ui, "redis-dialog-value");
                            ui.add(
                                TextEdit::multiline(text)
                                    .id(id)
                                    .desired_width(340.)
                                    .desired_rows(5),
                            );
                        }
                        Dialog::Rename { key, name } => {
                            ui.add(egui::Label::new(format!("原Key名: {key}")).wrap());
                            ui.label("新Key名：");
                            let id = view.dialog_editors[0].prepare(ui, "redis-dialog-main");
                            let response =
                                ui.add(TextEdit::singleline(name).id(id).desired_width(340.));
                            if focus {
                                response.request_focus();
                            }
                        }
                        Dialog::Ttl { key, seconds } => {
                            ui.add(egui::Label::new(format!("Key: {key}")).wrap());
                            ui.label("过期时间（秒），-1 表示永不过期:");
                            let id = view.dialog_editors[0].prepare(ui, "redis-dialog-main");
                            let response =
                                ui.add(TextEdit::singleline(seconds).id(id).desired_width(340.));
                            if focus {
                                response.request_focus();
                            }
                        }
                        Dialog::Delete { keys } => {
                            let message = if keys.len() == 1 {
                                format!("确定要删除Key \"{}\" 吗？", keys[0])
                            } else {
                                format!("确定要删除选中的 {} 个Key吗？", keys.len())
                            };
                            ui.add(egui::Label::new(message).wrap());
                            if (2..=10).contains(&keys.len()) {
                                ScrollArea::both().max_height(200.).show(ui, |ui| {
                                    ui.label("将删除以下Key：");
                                    for key in keys.iter() {
                                        ui.label(key);
                                    }
                                });
                            } else if keys.len() > 10 {
                                ui.label(format!("将删除 {} 个Key", keys.len()));
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        let accept = ui
                            .button(if matches!(dialog, Dialog::Delete { .. }) {
                                "删除"
                            } else {
                                "确定"
                            })
                            .clicked();
                        if accept {
                            match submit(dialog) {
                                Ok(Some(next)) => *action = Some(next),
                                Ok(None) => close = true,
                                Err(error) => validation = Some(error),
                            }
                        }
                        close |= ui.button("取消").clicked();
                    });
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
        close |= !open && !busy;
    }
    if close {
        view.dialog = None;
    }
    if view.dialog.is_none() {
        for editor in &view.dialog_editors {
            editor.release(ui.ctx());
        }
    }
    if let Some(text) = validation {
        view.failure = Some(Failure {
            title: "提示",
            text,
            focus: true,
        });
    }
    if let Some(failure) = &mut view.failure {
        let mut open = true;
        let mut dismiss = false;
        let focus = std::mem::take(&mut failure.focus);
        egui::Window::new(failure.title)
            .id(ui.id().with("redis-failure"))
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(420.)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(440.);
                ScrollArea::vertical().max_height(300.).show(ui, |ui| {
                    ui.add(egui::Label::new(&failure.text).wrap());
                });
                let response = ui.button("确定");
                if focus {
                    response.request_focus();
                }
                dismiss = response.clicked()
                    || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            });
        if !open || dismiss {
            view.failure = None;
            view.dialog_focus = view.dialog.is_some();
        }
    }
}

pub(super) fn submit(dialog: &Dialog) -> Result<Option<Action>, String> {
    Ok(Some(match dialog {
        Dialog::Add { key, kind, text } => {
            let key = key.trim();
            if key.is_empty() {
                return Ok(None);
            }
            tomato_services::redis_data::write_args(kind, text).map_err(|e| e.to_string())?;
            Action::Write {
                key: key.into(),
                kind: kind.clone(),
                text: text.clone(),
                create: false,
            }
        }
        Dialog::Rename { key, name } => {
            let name = name.trim();
            if name.is_empty() || name == key {
                return Ok(None);
            }
            Action::Rename {
                key: key.clone(),
                name: name.into(),
            }
        }
        Dialog::Ttl { key, seconds } => {
            let seconds: i64 = seconds
                .trim()
                .parse()
                .map_err(|_| "请输入有效的整数秒数".to_owned())?;
            if seconds < -1 {
                return Err("TTL 输入 -1 表示永久，其余须为非负秒数".into());
            }
            Action::Ttl {
                key: key.clone(),
                seconds,
            }
        }
        Dialog::Delete { keys } => Action::Delete { keys: keys.clone() },
    }))
}
