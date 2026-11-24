mod forms;
#[cfg(test)]
mod tests;
use super::{Intent, tools};
use crate::{
    tabs::game_dungeons::{Editor, State},
    theme,
};
use chrono::{Datelike, Local};
use egui::{Color32, RichText, ScrollArea, Sense, Stroke, Ui, vec2};
use tomato_core::game_dungeons::{Action, Board};

const RED: Color32 = Color32::from_rgb(211, 64, 64);
const GREEN: Color32 = Color32::from_rgb(20, 151, 83);
const ROW: f32 = 48.;
const ACCOUNT: f32 = 148.;
const CHARACTER: f32 = 148.;
const DUNGEON: f32 = 112.;
const PROGRESS: f32 = 84.;

pub(super) fn render(
    ui: &mut Ui,
    state: &mut State,
    busy: bool,
    error: Option<&str>,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    tools::header(
        ui,
        icons,
        "game_dungeons",
        "游戏副本管理",
        "账号与角色 · 每周副本完成情况",
    );
    let now = Local::now();
    let today = now.date_naive().num_days_from_ce();
    // One scheduled repaint at the next local midnight; no frame polling or disk reads.
    let delay = now
        .date_naive()
        .succ_opt()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|date| date.and_local_timezone(Local).earliest())
        .and_then(|next| (next - now).to_std().ok())
        .unwrap_or(std::time::Duration::from_secs(3600));
    ui.ctx()
        .request_repaint_after(delay + std::time::Duration::from_millis(100));
    let mut action = None;
    ui.add_space(10.);
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!busy && state.snapshot.is_some(), |ui| {
            if ui.button("添加账号").clicked() {
                state.new_account();
            }
            if ui
                .add_enabled(
                    state.board().is_some_and(|b| !b.accounts.is_empty()),
                    egui::Button::new("添加角色"),
                )
                .clicked()
            {
                state.new_character();
            }
            if ui.button("添加副本").clicked() {
                state.new_dungeon();
            }
            if ui.button("管理账号 / 角色 / 副本").clicked() {
                state.editor = Some(Editor::Manage);
            }
        });
        if ui
            .add_enabled(!busy, egui::Button::new("刷新"))
            .on_hover_text("重新读取本地记录；其他窗口修改后请先刷新")
            .clicked()
        {
            action = Some(Action::Load);
        }
        if busy {
            ui.spinner();
            ui.label("正在保存 / 读取…");
        }
    });
    if let Some(board) = state.board() {
        let accounts: Vec<_> = board
            .accounts
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            ui.label("账号");
            let selected = state
                .account_filter
                .as_ref()
                .and_then(|id| accounts.iter().find(|(key, _)| key == id))
                .map(|(_, name)| name.as_str())
                .unwrap_or("全部账号");
            egui::ComboBox::from_id_salt("dungeon-account-filter")
                .width(155.)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.account_filter, None, "全部账号");
                    for (id, name) in &accounts {
                        ui.selectable_value(&mut state.account_filter, Some(id.clone()), name);
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .desired_width(180.)
                    .hint_text("搜索账号或角色"),
            );
            ui.colored_label(GREEN, "■ 已打");
            ui.colored_label(RED, "■ 未打");
            ui.weak("点击方块切换 · 点击名称编辑");
        });
        ui.add_space(6.);
        let board = state.board().unwrap();
        let total = board.characters.len() * board.dungeons.len();
        let done = board
            .characters
            .iter()
            .map(|c| {
                board
                    .dungeons
                    .iter()
                    .filter(|d| board.completed(&c.id, d, today))
                    .count()
            })
            .sum::<usize>();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "全局：{} 个账号 · {} 个角色 · {} 个副本",
                board.accounts.len(),
                board.characters.len(),
                board.dungeons.len()
            ));
            ui.colored_label(GREEN, format!("已打 {done}"));
            ui.colored_label(RED, format!("未打 {}", total - done));
            ui.weak(format!(
                "{} · 本机时间，每周指定日 00:00 重置",
                now.format("%Y-%m-%d")
            ));
        });
        ui.add_space(8.);
        if board.accounts.is_empty() {
            ui.label("还没有账号。点击「添加账号」，再为账号添加角色和需要跟踪的副本。");
        } else if board.characters.is_empty() {
            ui.label("账号已添加。点击「添加角色」，可在同一账号下管理多个角色。");
        } else if board.dungeons.is_empty() {
            ui.label("点击「添加副本」，设置副本名称和每周更新日后即可在表格中打勾。");
        }
        let (edit, toggle) = matrix(
            ui,
            board,
            state.account_filter.as_deref(),
            &state.query,
            today,
            !busy,
        );
        if let Some(edit) = edit {
            state.editor = Some(edit);
        }
        if toggle.is_some() {
            action = toggle;
        }
    } else if !busy && state.load_requested {
        ui.label("记录未能读取，请点击「刷新」重试。");
    }
    if let Some(next) = forms::render(ui.ctx(), state, busy, error) {
        action = Some(next);
    }
    if !state.load_requested && !busy {
        action = Some(Action::Load);
    }
    action.map(|action| Intent::command(state.command(action)))
}

fn cell(ui: &mut Ui, rect: egui::Rect, text: &str, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        theme::java_font(12.),
        color,
    );
}
fn name_cell(ui: &mut Ui, rect: egui::Rect, text: &str, id: egui::Id, enabled: bool) -> bool {
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.), 0, theme::TREE_HOVER);
    }
    let galley = ui.painter().layout(
        text.to_owned(),
        theme::java_font(12.),
        Color32::from_gray(51),
        rect.width() - 16.,
    );
    ui.painter()
        .with_clip_rect(rect.shrink2(vec2(8., 2.)))
        .galley(
            egui::pos2(rect.left() + 8., rect.center().y - galley.size().y / 2.),
            galley,
            Color32::from_gray(51),
        );
    response
        .on_hover_text(format!("{text}\n点击编辑"))
        .clicked()
}

fn matrix(
    ui: &mut Ui,
    board: &Board,
    filter: Option<&str>,
    query: &str,
    today: i32,
    enabled: bool,
) -> (Option<Editor>, Option<Action>) {
    let query = query.trim().to_lowercase();
    let mut rows = Vec::new();
    for account in &board.accounts {
        if filter.is_some_and(|id| id != account.id) {
            continue;
        }
        for character in board
            .characters
            .iter()
            .filter(|c| c.account_id == account.id)
        {
            if query.is_empty()
                || character.name.to_lowercase().contains(&query)
                || account.name.to_lowercase().contains(&query)
            {
                rows.push((account, character));
            }
        }
    }
    let mut edit = None;
    let mut action = None;
    if rows.is_empty() && !board.characters.is_empty() {
        ui.weak("没有匹配的角色，请调整筛选条件。");
    }
    let width = ACCOUNT + CHARACTER + DUNGEON * board.dungeons.len() as f32 + PROGRESS;
    ScrollArea::horizontal()
        .id_salt("dungeon-columns")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(width);
            let (header, _) = ui.allocate_exact_size(vec2(width, 60.), Sense::hover());
            ui.painter()
                .rect_filled(header, 0, Color32::from_rgb(247, 248, 250));
            let rect = |offset: f32, width: f32| {
                egui::Rect::from_min_size(
                    header.min + vec2(offset, 0.),
                    vec2(width, header.height()),
                )
            };
            cell(ui, rect(0., ACCOUNT), "账号", Color32::from_gray(80));
            cell(ui, rect(ACCOUNT, CHARACTER), "角色", Color32::from_gray(80));
            for (index, dungeon) in board.dungeons.iter().enumerate() {
                let area = rect(ACCOUNT + CHARACTER + index as f32 * DUNGEON, DUNGEON);
                if !ui.is_rect_visible(area) {
                    continue;
                }
                let response = ui.interact(
                    area,
                    ui.id().with(("dungeon", &dungeon.id)),
                    if enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                let label = egui::Label::new(RichText::new(&dungeon.name).size(12.)).truncate();
                tools::place_left(
                    ui,
                    area.shrink2(vec2(8., 0.)).with_max_y(area.top() + 30.),
                    label,
                );
                let next = dungeon.reset_day.period_start(today) + 7;
                let next = chrono::NaiveDate::from_num_days_from_ce_opt(next)
                    .map(|d| d.format("%m-%d").to_string())
                    .unwrap_or_default();
                cell(
                    ui,
                    area.with_min_y(area.top() + 28.),
                    &format!("{}更新 · {next}", dungeon.reset_day.label()),
                    Color32::from_gray(125),
                );
                if response
                    .on_hover_text(format!(
                        "{}\n每{} 00:00 重置\n点击编辑副本",
                        dungeon.name,
                        dungeon.reset_day.label()
                    ))
                    .clicked()
                {
                    edit = Some(Editor::Dungeon {
                        id: Some(dungeon.id.clone()),
                        name: dungeon.name.clone(),
                        reset_day: dungeon.reset_day,
                    });
                }
            }
            cell(
                ui,
                rect(width - PROGRESS, PROGRESS),
                "完成进度",
                Color32::from_gray(80),
            );
            ui.spacing_mut().item_spacing.y = 0.;
            ScrollArea::vertical()
                .id_salt("dungeon-rows")
                .auto_shrink([false, false])
                .show_rows(ui, ROW, rows.len(), |ui, range| {
                    for row in range {
                        let (account, character) = rows[row];
                        let (area, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                        if row % 2 == 1 {
                            ui.painter().rect_filled(area, 0, Color32::from_gray(251));
                        }
                        let rect = |offset: f32, width: f32| {
                            egui::Rect::from_min_size(area.min + vec2(offset, 0.), vec2(width, ROW))
                        };
                        if name_cell(
                            ui,
                            rect(0., ACCOUNT),
                            &account.name,
                            ui.id().with(("account", row)),
                            enabled,
                        ) {
                            edit = Some(Editor::Account {
                                id: Some(account.id.clone()),
                                name: account.name.clone(),
                            });
                        }
                        if name_cell(
                            ui,
                            rect(ACCOUNT, CHARACTER),
                            &character.name,
                            ui.id().with(("character", &character.id)),
                            enabled,
                        ) {
                            edit = Some(Editor::Character {
                                id: Some(character.id.clone()),
                                account_id: account.id.clone(),
                                name: character.name.clone(),
                            });
                        }
                        let mut done = 0;
                        for (index, dungeon) in board.dungeons.iter().enumerate() {
                            let completed = board.completed(&character.id, dungeon, today);
                            done += usize::from(completed);
                            let area = rect(ACCOUNT + CHARACTER + index as f32 * DUNGEON, DUNGEON);
                            if !ui.is_rect_visible(area) {
                                continue;
                            }
                            let square =
                                egui::Rect::from_center_size(area.center(), vec2(34., 34.));
                            let response = ui.interact(
                                square,
                                ui.id().with((&character.id, &dungeon.id)),
                                if enabled {
                                    Sense::click()
                                } else {
                                    Sense::hover()
                                },
                            );
                            let color = if completed { GREEN } else { RED };
                            ui.painter().rect_filled(square, 3, color);
                            if response.hovered() {
                                ui.painter().rect_stroke(
                                    square.expand(2.),
                                    4,
                                    Stroke::new(1., color),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            cell(
                                ui,
                                square,
                                if completed { "✓" } else { "未" },
                                Color32::WHITE,
                            );
                            if response
                                .on_hover_text(format!(
                                    "{} / {}\n{}：{}\n点击标记为{}",
                                    account.name,
                                    character.name,
                                    dungeon.name,
                                    if completed { "已打" } else { "未打" },
                                    if completed { "未打" } else { "已打" }
                                ))
                                .clicked()
                            {
                                action = Some(Action::SetCompleted {
                                    character_id: character.id.clone(),
                                    dungeon_id: dungeon.id.clone(),
                                    completed: !completed,
                                    period_start: dungeon.reset_day.period_start(today),
                                });
                            }
                        }
                        cell(
                            ui,
                            rect(width - PROGRESS, PROGRESS),
                            &format!("{done}/{}", board.dungeons.len()),
                            if done > 0 && done == board.dungeons.len() {
                                GREEN
                            } else {
                                Color32::from_gray(100)
                            },
                        );
                        ui.painter().hline(
                            area.x_range(),
                            area.bottom(),
                            Stroke::new(1., theme::DIVIDER),
                        );
                        for offset in [ACCOUNT, ACCOUNT + CHARACTER, width - PROGRESS] {
                            ui.painter().vline(
                                area.left() + offset,
                                area.y_range(),
                                Stroke::new(1., theme::DIVIDER),
                            );
                        }
                    }
                });
        });
    (edit, action)
}
