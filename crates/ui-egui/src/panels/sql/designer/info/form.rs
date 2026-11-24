//! Inline detail editor below the fixed-height metadata grid.
use super::*;
pub(super) fn show(
    ui: &mut Ui,
    state: &mut crate::tabs::table_design::info::State,
    before: &[Vec<String>],
    index: usize,
    dialect: tomato_core::table_design::Dialect,
    writable: bool,
) -> (bool, Vec<egui::Id>) {
    let Some(row) = state.form else {
        return (false, Vec::new());
    };
    let editable = writable && state.added.contains(&row);
    let headers = tomato_core::table_design::auxiliary::headers(if index == 1 {
        tomato_core::table_design::alter::InfoPage::ForeignKeys
    } else {
        tomato_core::table_design::alter::InfoPage::Triggers
    });
    let ids: Vec<_> = (0..7)
        .map(|col| ui.id().with(("aux-input", index, col)))
        .collect();
    let fresh = std::mem::take(&mut state.focus_form);
    if fresh {
        ui.ctx().data_mut(|data| {
            for id in &ids {
                data.remove::<egui::widgets::text_edit::TextEditState>(*id);
            }
        });
    }
    let mut owned_ids = ids.clone();
    let mut changed = false;
    let bounds = ui.max_rect();
    let height = (bounds.height() - 36.).max(0.);
    egui::ScrollArea::vertical()
        .id_salt(("aux-form", index))
        .auto_shrink([false, false])
        .max_height(height)
        .min_scrolled_height(0.)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(8., 6.);
            ui.label(if editable {
                "填写定义后点击顶部“保存”应用到数据库。"
            } else {
                "已有定义只读；可删除后重新添加，未修改的其他属性会保留。"
            });
            for (column, label) in headers.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        vec2(85., 24.),
                        egui::Label::new(label).halign(egui::Align::Min),
                    );
                    let width = ui.available_width().max(30.);
                    let height = if index == 2 && column == 3 { 90. } else { 26. };
                    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                    if !editable {
                        let values = state.rows.as_deref().unwrap_or(before);
                        if let Some(value) = values.get(row).and_then(|r| r.get(column)) {
                            let mut value = value.as_str();
                            let text = if index == 2 && column == 3 {
                                TextEdit::multiline(&mut value)
                            } else {
                                TextEdit::singleline(&mut value)
                            };
                            ui.put(
                                rect,
                                text.desired_rows(1)
                                    .id(ids[column])
                                    .font(crate::native_dialog::font(ui.ctx(), 12.)),
                            );
                        }
                        return;
                    }
                    let Some(value) = state
                        .rows
                        .as_mut()
                        .and_then(|rows| rows.get_mut(row))
                        .and_then(|r| r.get_mut(column))
                    else {
                        return;
                    };
                    let items: &[&str] = match (index, column) {
                        (1, 6) if matches!(dialect, tomato_core::table_design::Dialect::Oracle) => {
                            &["NO ACTION"]
                        }
                        (1, 5) if matches!(dialect, tomato_core::table_design::Dialect::Oracle) => {
                            &["NO ACTION", "CASCADE", "SET NULL"]
                        }
                        (1, 5 | 6) => &[
                            "NO ACTION",
                            "RESTRICT",
                            "CASCADE",
                            "SET NULL",
                            "SET DEFAULT",
                        ],
                        (2, 1)
                            if matches!(
                                dialect,
                                tomato_core::table_design::Dialect::Mysql
                                    | tomato_core::table_design::Dialect::Oracle
                            ) =>
                        {
                            &["BEFORE", "AFTER"]
                        }
                        (2, 1) => &["BEFORE", "AFTER", "INSTEAD OF"],
                        (2, 2) if matches!(dialect, tomato_core::table_design::Dialect::Oracle) => {
                            &[
                                "INSERT",
                                "UPDATE",
                                "DELETE",
                                "INSERT OR UPDATE",
                                "INSERT OR DELETE",
                                "UPDATE OR DELETE",
                                "INSERT OR UPDATE OR DELETE",
                            ]
                        }
                        (2, 2) => &[
                            "INSERT",
                            "UPDATE",
                            "DELETE",
                            "INSERT OR UPDATE",
                            "INSERT OR DELETE",
                            "UPDATE OR DELETE",
                            "INSERT OR UPDATE OR DELETE",
                            "TRUNCATE",
                        ],
                        _ => &[],
                    };
                    if !items.is_empty() {
                        let name = format!("aux-choice-{index}-{column}");
                        let id = ui.id().with((name.as_str(), "text"));
                        owned_ids.push(id);
                        if fresh {
                            ui.ctx().data_mut(|data| {
                                data.remove::<egui::widgets::text_edit::TextEditState>(id)
                            });
                        }
                        changed |= super::super::pages::combo(ui, rect, &name, value, items);
                    } else {
                        let text = if index == 2 && column == 3 {
                            TextEdit::multiline(value)
                        } else {
                            TextEdit::singleline(value)
                        };
                        let response = ui.put(
                            rect,
                            text.id(ids[column])
                                .font(crate::native_dialog::font(ui.ctx(), 12.))
                                .char_limit(128 * 1024),
                        );
                        if fresh && column == 0 {
                            response.request_focus();
                        }
                        changed |= response.changed();
                    }
                });
            }
            ui.label(if index == 1 {
                "复合字段以逗号分隔；名称含逗号时用双引号或反引号包围。"
            } else if matches!(dialect, tomato_core::table_design::Dialect::Postgres) {
                "语句填写触发函数调用，例如 public.audit_row()。"
            } else if matches!(dialect, tomato_core::table_design::Dialect::Oracle) {
                "语句填写 PL/SQL 触发体，例如 BEGIN :NEW.\"label\" := 'value'; END;"
            } else {
                "语句填写触发体，例如 SET NEW.updated_at = NOW()，也支持 BEGIN … END。"
            });
        });
    let close = ui
        .put(
            Rect::from_min_size(pos2(bounds.left(), bounds.bottom() - 28.), vec2(52., 26.)),
            egui::Button::new("收起"),
        )
        .clicked();
    if close {
        state.form = None;
        ui.ctx().data_mut(|data| {
            for id in ids {
                data.remove::<egui::widgets::text_edit::TextEditState>(id);
            }
        });
    }
    (changed, owned_ids)
}
