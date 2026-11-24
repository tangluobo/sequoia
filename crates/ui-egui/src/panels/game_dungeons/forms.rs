use crate::tabs::game_dungeons::{Editor, State};
use egui::{Context, RichText, ScrollArea, Ui};
use tomato_core::game_dungeons::{Action, Board, Weekday};

fn name(ui: &mut Ui, value: &mut String, label: &str) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(340.)
            .char_limit(120),
    );
}
fn delete(ui: &mut Ui, action: Action, description: String, next: &mut Option<Editor>) {
    if ui.button("删除").clicked() {
        *next = Some(Editor::Delete {
            action,
            description,
        });
    }
}
pub(super) fn render(
    ctx: &Context,
    state: &mut State,
    busy: bool,
    error: Option<&str>,
) -> Option<Action> {
    let mut editor = state.editor.take()?;
    let board = state.board()?;
    let mut action = None;
    let mut next = None;
    let mut close = false;
    let response = egui::Modal::new(egui::Id::new("game-dungeons-editor")).show(ctx, |ui| {
        ui.set_width(420.);
        ui.add_enabled_ui(!busy, |ui| {
            match &mut editor {
                Editor::Account { id, name: value } => {
                    ui.heading(if id.is_some() {
                        "编辑账号"
                    } else {
                        "添加账号"
                    });
                    ui.add_space(8.);
                    name(ui, value, "账号名称（用于区分，无需密码）");
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!value.trim().is_empty(), egui::Button::new("保存"))
                            .clicked()
                        {
                            action = Some(Action::SaveAccount {
                                id: id.clone(),
                                name: value.clone(),
                            });
                        }
                        if let Some(id) = id {
                            let count = board
                                .characters
                                .iter()
                                .filter(|c| &c.account_id == id)
                                .count();
                            delete(
                                ui,
                                Action::DeleteAccount { id: id.clone() },
                                format!("删除账号「{value}」及其 {count} 个角色和全部完成记录？"),
                                &mut next,
                            );
                        }
                    });
                }
                Editor::Character {
                    id,
                    account_id,
                    name: value,
                } => {
                    ui.heading(if id.is_some() {
                        "编辑角色"
                    } else {
                        "添加角色"
                    });
                    ui.add_space(8.);
                    ui.label("所属账号");
                    let selected = board
                        .accounts
                        .iter()
                        .find(|a| &a.id == account_id)
                        .map(|a| a.name.as_str())
                        .unwrap_or("请选择账号");
                    egui::ComboBox::from_id_salt("character-account")
                        .width(340.)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for account in &board.accounts {
                                ui.selectable_value(account_id, account.id.clone(), &account.name);
                            }
                        });
                    name(ui, value, "角色名称");
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !value.trim().is_empty() && !account_id.is_empty(),
                                egui::Button::new("保存"),
                            )
                            .clicked()
                        {
                            action = Some(Action::SaveCharacter {
                                id: id.clone(),
                                account_id: account_id.clone(),
                                name: value.clone(),
                            });
                        }
                        if let Some(id) = id {
                            delete(
                                ui,
                                Action::DeleteCharacter { id: id.clone() },
                                format!("删除角色「{value}」及其全部完成记录？"),
                                &mut next,
                            );
                        }
                    });
                }
                Editor::Dungeon {
                    id,
                    name: value,
                    reset_day,
                } => {
                    ui.heading(if id.is_some() {
                        "编辑副本"
                    } else {
                        "添加副本"
                    });
                    ui.add_space(8.);
                    name(ui, value, "副本名称");
                    ui.label("每周更新日");
                    egui::ComboBox::from_id_salt("dungeon-reset-day")
                        .width(180.)
                        .selected_text(reset_day.label())
                        .show_ui(ui, |ui| {
                            for day in Weekday::ALL {
                                ui.selectable_value(reset_day, day, day.label());
                            }
                        });
                    ui.weak("每周当天 00:00 自动恢复为红色未打（本机时间）。");
                    if let Some(id) = id
                        && board
                            .dungeons
                            .iter()
                            .find(|d| &d.id == id)
                            .is_some_and(|d| d.reset_day != *reset_day)
                    {
                        ui.colored_label(super::RED, "修改更新日会清除该副本的所有完成标记。");
                    }
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!value.trim().is_empty(), egui::Button::new("保存"))
                            .clicked()
                        {
                            action = Some(Action::SaveDungeon {
                                id: id.clone(),
                                name: value.clone(),
                                reset_day: *reset_day,
                            });
                        }
                        if let Some(id) = id {
                            delete(
                                ui,
                                Action::DeleteDungeon { id: id.clone() },
                                format!("删除副本「{value}」及所有角色对应的完成记录？"),
                                &mut next,
                            );
                        }
                    });
                }
                Editor::Manage => manage(ui, board, &mut next),
                Editor::Delete {
                    action: deletion,
                    description,
                } => {
                    ui.heading("确认删除");
                    ui.add_space(10.);
                    ui.label(description.as_str());
                    ui.weak("删除后无法恢复。");
                    ui.add_space(14.);
                    if ui
                        .button(RichText::new("确认删除").color(super::RED))
                        .clicked()
                    {
                        action = Some(deletion.clone());
                    }
                }
            }
            ui.add_space(8.);
            if ui
                .button(if matches!(editor, Editor::Manage) {
                    "关闭"
                } else {
                    "取消"
                })
                .clicked()
            {
                close = true;
            }
        });
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("正在保存…");
            });
        }
        if let Some(error) = error {
            ui.colored_label(super::RED, error);
        }
    });
    if !(close || response.should_close()) || busy {
        state.editor = Some(next.unwrap_or(editor));
    }
    action
}

fn manage(ui: &mut Ui, board: &Board, next: &mut Option<Editor>) {
    ui.heading("账号、角色与副本");
    ui.weak("点击名称编辑；删除账号会同时删除其角色及完成记录。");
    ScrollArea::vertical()
        .id_salt("dungeon-management")
        .max_height(420.)
        .show(ui, |ui| {
            for account in &board.accounts {
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(&account.name).strong()).clicked() {
                        *next = Some(Editor::Account {
                            id: Some(account.id.clone()),
                            name: account.name.clone(),
                        });
                    }
                    if ui.small_button("添加角色").clicked() {
                        *next = Some(Editor::Character {
                            id: None,
                            account_id: account.id.clone(),
                            name: String::new(),
                        });
                    }
                });
                ui.indent(&account.id, |ui| {
                    for character in board
                        .characters
                        .iter()
                        .filter(|c| c.account_id == account.id)
                    {
                        if ui.button(&character.name).clicked() {
                            *next = Some(Editor::Character {
                                id: Some(character.id.clone()),
                                account_id: account.id.clone(),
                                name: character.name.clone(),
                            });
                        }
                    }
                    if !board.characters.iter().any(|c| c.account_id == account.id) {
                        ui.weak("暂无角色");
                    }
                });
            }
            ui.separator();
            ui.label("副本设置");
            for dungeon in &board.dungeons {
                if ui
                    .button(format!(
                        "{} · {}更新",
                        dungeon.name,
                        dungeon.reset_day.label()
                    ))
                    .clicked()
                {
                    *next = Some(Editor::Dungeon {
                        id: Some(dungeon.id.clone()),
                        name: dungeon.name.clone(),
                        reset_day: dungeon.reset_day,
                    });
                }
            }
            if board.dungeons.is_empty() {
                ui.weak("暂无副本");
            }
        });
}
