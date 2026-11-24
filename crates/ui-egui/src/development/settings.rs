use super::*;
use egui::{Ui, vec2};
mod add_directory;
mod controls;
mod draft;
mod entry;
mod manager;
mod preview;
mod project;
pub(super) use add_directory::{Draft as DirectoryDraft, result as directory_result};
pub(super) use draft::Draft;
pub(super) fn dialogs(ctx: &egui::Context, state: &mut State, icons: &mut Icons) {
    let busy = state
        .pending
        .values()
        .any(|r| matches!(r, Request::Config { .. }));
    add_directory::show(ctx, state, icons);
    project::dialogs(ctx, state, busy, icons);
    if let Some(confirm) = state.confirm.take() {
        let title = match &confirm {
            Confirm::Navigate(_) => "未保存的修改",
            Confirm::Rollback(_) => "回滚变更",
            Confirm::Push => "推送到远程仓库",
        };
        let mut accepted = false;
        let mut cancel = false;
        let mut save = false;
        let dismissed = dialog(
            ctx,
            "development-confirm",
            title,
            vec2(480., 190.),
            false,
            |ui| {
                match &confirm {
                    Confirm::Navigate(_) => {
                        ui.label("文件有未保存的修改，是否保存？");
                    }
                    Confirm::Rollback(files) => {
                        ui.label(format!(
                            "确定回滚选中的 {} 个文件吗？未添加的文件将被删除，此操作不可撤销。",
                            files.len()
                        ));
                    }
                    Confirm::Push => {
                        ui.label("远程仓库：");
                        ui.text_edit_singleline(&mut state.push_remote);
                        ui.label("分支：");
                        ui.text_edit_singleline(&mut state.push_branch);
                        ui.label(format!(
                            "提交并推送 {} 个选中文件",
                            state.selected_changes.len()
                        ));
                    }
                }
                ui.horizontal(|ui| {
                    if matches!(confirm, Confirm::Navigate(_)) {
                        save = ui.button("保存").clicked();
                    }
                    accepted = ui
                        .button(if matches!(confirm, Confirm::Navigate(_)) {
                            "不保存"
                        } else {
                            "确定"
                        })
                        .clicked();
                    cancel = ui.button("取消").clicked();
                });
            },
        );
        cancel |= dismissed;
        if save {
            if let Confirm::Navigate(next) = confirm {
                state.save_file(Some(next));
            }
        } else if accepted {
            match confirm {
                Confirm::Navigate(next) => state.navigate_now(next),
                Confirm::Rollback(files) => {
                    if let Some(root) = state.root() {
                        state.ordinary(Action::Git {
                            root,
                            action: service::GitAction::Rollback { files },
                        });
                    }
                }
                Confirm::Push => view::commit_changes(
                    state,
                    Some(service::Push {
                        remote: state.push_remote.clone(),
                        branch: state.push_branch.clone(),
                    }),
                ),
            }
        } else if !cancel {
            state.confirm = Some(confirm)
        }
    }
    if let Some(error) = state.error.clone() {
        let mut close = false;
        let dismissed = dialog(
            ctx,
            "development-error",
            "操作失败",
            vec2(480., 160.),
            false,
            |ui| {
                ui.label(&error);
                close = ui.button("确定").clicked();
            },
        );
        if close || dismissed {
            state.error = None;
        }
    }
    if let Some(notice) = state.notice.clone() {
        let mut close = false;
        let dismissed = dialog(
            ctx,
            "development-notice",
            "操作完成",
            vec2(480., 160.),
            false,
            |ui| {
                ui.label(&notice);
                close = ui.button("确定").clicked();
            },
        );
        if close || dismissed {
            state.notice = None;
        }
    }
}

fn dialog(
    ctx: &egui::Context,
    key: &str,
    title: &str,
    size: egui::Vec2,
    resizable: bool,
    mut body: impl FnMut(&mut Ui),
) -> bool {
    let mut dismissed = false;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(key),
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_resizable(resizable)
            .with_taskbar(false),
        |ui, _| {
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .inner_margin(16)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8., 10.);
                    body(ui);
                });
            if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                dismissed = true;
            }
        },
    );
    dismissed
}
fn native(
    ctx: &egui::Context,
    key: &str,
    title: &str,
    size: egui::Vec2,
    owner: Option<egui::Rect>,
    mut body: impl FnMut(&mut Ui),
) -> bool {
    let mut dismissed = false;
    let mut builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_resizable(false)
        .with_taskbar(false);
    if let Some(owner) = owner {
        builder = builder.with_position(owner.center() - size * 0.5);
    }
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(key), builder, |ui, _| {
        ui.painter()
            .rect_filled(ui.max_rect(), 0, egui::Color32::from_gray(244));
        body(ui);
        if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            dismissed = true;
        }
    });
    dismissed
}
