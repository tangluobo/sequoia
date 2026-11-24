mod batch;
mod context_actions;
mod context_menu;
mod criteria_view;
mod editor;
mod grid;
mod projection;
mod status;
pub(super) mod toolbar;
mod transfer;
use super::*;
use crate::tabs::workbench::{CellEditor, Workbench};
use editor::set_value;
use grid::grid;
use tomato_core::workbench::*;
use transfer::copy;

fn command(connection: &tomato_core::Connection, action: DatabaseAction) -> Intent {
    Intent::command(Command::Database {
        connection: connection.clone(),
        action,
    })
}

pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    let Panel::Sql {
        connection,
        workbench: state,
        ..
    } = &mut tab.panel
    else {
        return None;
    };
    let mut intent = None;
    let writable = state
        .definition
        .as_ref()
        .is_some_and(|definition| !definition.view && definition.transactional)
        && !state.needs_refresh
        && !state.session_lost
        && !tab.busy;
    let primary = state.definition.as_ref().is_some_and(|definition| {
        definition
            .columns
            .iter()
            .any(|column| column.primary_key > 0)
    });
    ui.spacing_mut().item_spacing.y = 0.;
    let bounds = ui.available_rect_before_wrap();
    ui.painter().rect_filled(bounds, 0, Color32::WHITE);
    egui::Frame::new()
        .fill(Color32::from_gray(243))
        .inner_margin(egui::Margin::symmetric(5, 2))
        .show(ui, |ui| {
            ui.set_min_width((bounds.width() - 10.).max(0.));
            ui.set_min_height(31.);
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
            ui.spacing_mut().item_spacing.x = 3.;
            ui.horizontal(|ui| {
                ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                ui.spacing_mut().button_padding = egui::vec2(5., 3.);
                toolbar::menu(ui, 0, "开始事务", |ui| {
                    for (action, label) in [
                        (TransactionAction::Begin, "开始事务"),
                        (TransactionAction::Commit, "提交事务"),
                        (TransactionAction::Rollback, "回滚事务"),
                    ] {
                        if ui
                            .add_enabled(
                                !tab.busy
                                    && connection.kind() != "HIVE"
                                    && match action {
                                        TransactionAction::Begin => !state.transaction,
                                        TransactionAction::Commit | TransactionAction::Rollback => {
                                            state.transaction
                                        }
                                    },
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            intent =
                                Some(command(connection, DatabaseAction::Transaction { action }));
                            ui.close();
                        }
                    }
                });
                toolbar::separator(ui);
                toolbar::menu(ui, 1, "文本", |ui| {
                    for (label, format) in [("INSERT SQL", 0), ("JSON", 1), ("文本", 2)] {
                        if ui.button(label).clicked() {
                            match copy(state, connection.kind(), format) {
                                Ok(value) => ui.ctx().copy_text(value),
                                Err(error) => state.status = error.to_string(),
                            }
                            ui.close();
                        }
                    }
                });
                toolbar::separator(ui);
                toolbar::menu(ui, 2, "筛选", |ui| {
                    ui.add_enabled_ui(!tab.busy, |ui| criteria_view::show(ui, state, false));
                });
                toolbar::separator(ui);
                toolbar::menu(ui, 3, "排序", |ui| {
                    ui.add_enabled_ui(!tab.busy, |ui| criteria_view::show(ui, state, true));
                });
                toolbar::separator(ui);
                if ui
                    .add_enabled(
                        writable,
                        egui::Button::image_and_text(toolbar::action_icon(ui, 4), "导入")
                            .frame(false)
                            .min_size(egui::vec2(54., 31.)),
                    )
                    .on_hover_text("导入向导：选择来源、预览并映射字段后写入数据库")
                    .clicked()
                {
                    intent = Some(Intent::ImportData);
                }
                if ui
                    .add(
                        egui::Button::image_and_text(toolbar::action_icon(ui, 5), "导出")
                            .frame(false)
                            .min_size(egui::vec2(54., 31.)),
                    )
                    .on_hover_text("导出全部或当前记录")
                    .clicked()
                {
                    intent = Some(Intent::ExportData(None));
                }
            });
        });
    ui.painter().hline(
        bounds.x_range(),
        ui.cursor().top() - 1.,
        egui::Stroke::new(1., Color32::from_gray(214)),
    );
    {
        if state.needs_refresh {
            ui.colored_label(
                Color32::RED,
                if !state.status.is_empty() {
                    state.status.as_str()
                } else if state.dirty() {
                    "表数据已变化，未保存的编辑已保留，请核对后刷新"
                } else {
                    "修改已保存，请刷新后继续编辑"
                },
            );
        }
        ui.push_id(("table-data", &tab.id), |ui| {
            grid(ui, state, writable, connection.kind())
        });
        egui::Frame::new()
            .fill(Color32::from_gray(243))
            .inner_margin(egui::Margin {
                left: 9,
                right: 5,
                top: 1,
                bottom: 1,
            })
            .show(ui, |ui| {
                ui.set_min_width((bounds.width() - 14.).max(0.));
                ui.set_min_height(25.);
                ui.spacing_mut().button_padding = egui::vec2(5., 2.);
                ui.spacing_mut().interact_size.y = 22.;
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(11.));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if toolbar::button(ui, writable && state.inserted.len() < 1000, 0, "添加")
                        .clicked()
                        && let Some(definition) = &state.definition
                    {
                        state.inserted.push(vec![None; definition.columns.len()]);
                    }
                    if toolbar::button(
                        ui,
                        writable && primary && !state.selected.is_empty(),
                        1,
                        "删除",
                    )
                    .clicked()
                    {
                        state.confirm_delete = true;
                    }
                    if toolbar::button(ui, writable && state.dirty(), 2, "保存").clicked()
                        && editor::finish(ui.ctx(), state, connection.kind(), editor::End::Commit)
                        && let Some(request) = state.request()
                    {
                        intent = Some(command(
                            connection,
                            DatabaseAction::Apply {
                                table: request.table.clone(),
                                changes: state.changes(),
                                request,
                            },
                        ));
                    }
                    if toolbar::button(ui, !tab.busy && state.dirty(), 3, "取消修改").clicked()
                    {
                        state.clear_changes();
                    }
                    let (separator, _) =
                        ui.allocate_exact_size(egui::vec2(14., 23.), egui::Sense::hover());
                    ui.painter().vline(
                        separator.center().x,
                        separator.y_range(),
                        egui::Stroke::new(1., Color32::from_gray(215)),
                    );
                    if toolbar::button(ui, !tab.busy, 4, "刷新").clicked() {
                        state.pending_page = state.request();
                    }
                    let paging_width = 124.;
                    ui.add_space((ui.available_width() - paging_width).max(0.));
                    pagination(ui, state, !tab.busy);
                });
            });
        status::render(ui, state, connection);
    }
    editor::picker(ui.ctx(), state);
    context_actions::dialog(ui.ctx(), state);
    if state.confirm_delete {
        egui::Modal::new(egui::Id::new(("delete-rows", &tab.id))).show(ui.ctx(), |ui| {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                state.confirm_delete = false;
                return;
            }
            ui.heading("删除行");
            ui.label(format!(
                "确定删除{}条数据？此操作不可撤销！",
                state.selected.len()
            ));
            ui.horizontal(|ui| {
                if ui.button("确定").clicked() {
                    let mut rows = Vec::new();
                    if let Some(page) = &state.page {
                        for &index in state.selected.iter().rev() {
                            if index < page.rows.len() {
                                rows.push((index, page.rows[index].clone()));
                            } else if index - page.rows.len() < state.inserted.len() {
                                state.inserted.remove(index - page.rows.len());
                                state
                                    .table_view
                                    .projection
                                    .order
                                    .retain(|row| *row != index);
                                for row in &mut state.table_view.projection.order {
                                    if *row > index {
                                        *row -= 1;
                                    }
                                }
                            }
                        }
                        if !rows.is_empty() {
                            intent = Some(command(
                                connection,
                                DatabaseAction::DeleteRows {
                                    table: page.definition.table.clone(),
                                    rows,
                                },
                            ));
                        }
                    }
                    state.table_view.selector_rows.clear();
                    state.table_view.row_mode = false;
                    state.table_view.selector_focus = false;
                    state.table_view.row_anchor = None;
                    state.table_view.cells.clear();
                    state.table_view.focus = None;
                    state.table_view.anchor = None;
                    state.selected.clear();
                    state.confirm_delete = false;
                }
                if ui.button("取消").clicked() {
                    state.confirm_delete = false;
                }
            });
        });
    }
    if state.pending_page.is_some() && !tab.busy {
        if state.dirty() {
            egui::Window::new("未保存修改")
                .id(egui::Id::new(("discard-rows", &tab.id)))
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("当前表有未保存修改，是否放弃并重新加载？");
                    if ui.button("放弃修改").clicked() {
                        state.clear_changes();
                    }
                    if ui.button("取消").clicked() {
                        state.pending_page = None;
                        state.search = state.query.search.clone();
                    }
                });
        } else if let Some(request) = state.pending_page.take() {
            intent = Some(command(connection, DatabaseAction::Page { request }));
        }
    }
    if intent.is_none()
        && let Some((path, text)) = state.table_view.projection.pending_write.take()
    {
        intent = Some(Intent::command(Command::WriteText { path, text }));
    }
    if state.table_view.export_requested {
        state.table_view.export_requested = false;
        intent = Some(Intent::ExportData(None));
    }
    intent
}

fn pagination(ui: &mut Ui, state: &mut Workbench, enabled: bool) {
    let Some(page) = &state.page else {
        return;
    };
    let (offset, limit, total) = (page.offset, u64::from(page.limit).max(1), page.total);
    if state.table_view.jump_offset != Some(offset) {
        state.table_view.jump = (offset / limit + 1).to_string();
        state.table_view.jump_offset = Some(offset);
    }
    for (index, (label, destination, available, hint)) in [
        ("|<", 0, offset > 0, "首页"),
        ("<", offset.saturating_sub(limit), offset > 0, "上一页"),
        (
            ">",
            offset.saturating_add(limit),
            offset.saturating_add(limit) < total,
            "下一页",
        ),
        (
            ">|",
            total.saturating_sub(1) / limit * limit,
            offset.saturating_add(limit) < total,
            "尾页",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            let response = ui.add_sized(
                [30., 19.],
                TextEdit::singleline(&mut state.table_view.jump)
                    .font(egui::FontId::proportional(11.))
                    .horizontal_align(egui::Align::Center),
            );
            if response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Ok(number) = state.table_view.jump.trim().parse::<u64>()
                && number >= 1
                && number <= total.div_ceil(limit).max(1)
                && let Some(mut request) = state.request()
            {
                request.offset = (number - 1) * limit;
                state.pending_page = Some(request);
            }
        }
        if ui
            .add_enabled(
                enabled && available,
                egui::Button::new(egui::RichText::new(label).size(10.))
                    .frame(false)
                    .min_size(egui::vec2(
                        if index == 0 || index == 3 { 23. } else { 20. },
                        21.,
                    )),
            )
            .on_hover_text(hint)
            .clicked()
            && let Some(mut request) = state.request()
        {
            request.offset = destination;
            state.pending_page = Some(request);
        }
    }
}

#[cfg(test)]
mod context_tests;

pub(super) fn export_snapshot(
    ctx: &egui::Context,
    state: &mut Workbench,
    dialect: &str,
) -> anyhow::Result<tomato_core::data_export::Snapshot> {
    anyhow::ensure!(
        editor::finish(ctx, state, dialect, editor::End::Commit),
        "请先修正正在编辑的单元格"
    );
    let page = state
        .page
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("表数据尚未加载"))?;
    let rows = projection::rows(state)
        .into_iter()
        .filter(|row| !state.deleted.contains(row))
        .map(|row| {
            (0..page.definition.columns.len())
                .map(|column| {
                    projection::value(state, row, column)
                        .cloned()
                        .unwrap_or(Cell::Null)
                })
                .collect()
        })
        .collect();
    Ok(tomato_core::data_export::Snapshot {
        schema: tomato_core::data_export::Schema {
            table: page.definition.table.clone(),
            dialect: dialect.into(),
            columns: page.definition.columns.clone(),
        },
        rows,
    })
}
