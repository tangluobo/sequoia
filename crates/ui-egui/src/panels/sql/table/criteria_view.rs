//! Explicit Apply keeps draft criteria separate from the last executed SQL/page.
use super::*;
use tomato_core::result_filter::{Condition, Operator};

pub(super) fn show(ui: &mut Ui, state: &mut Workbench, sorting: bool) {
    let Some(definition) = &state.definition else {
        return;
    };
    let draft = state
        .table_view
        .query_draft
        .get_or_insert_with(|| state.query.clone());
    ui.set_width(430.);
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let mut apply = false;
    if !sorting {
        let input = ui.add(
            TextEdit::singleline(&mut draft.search)
                .hint_text("快速筛选整个表")
                .desired_width(420.),
        );
        apply = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            ui.label("匹配");
            ui.selectable_value(&mut draft.match_any, false, "全部条件（AND）");
            ui.selectable_value(&mut draft.match_any, true, "任一条件（OR）");
        });
    }
    let mut remove = None;
    let mut move_sort = None;
    egui::ScrollArea::vertical()
        .max_height(300.)
        .show(ui, |ui| {
            if sorting {
                for (index, sort) in draft.sorts.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        field(ui, index, &mut sort.column, definition);
                        egui::ComboBox::from_id_salt(("direction", index))
                            .selected_text(if sort.descending { "降序" } else { "升序" })
                            .width(65.)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut sort.descending, false, "升序");
                                ui.selectable_value(&mut sort.descending, true, "降序");
                            });
                        if ui
                            .add_enabled(index > 0, egui::Button::new("↑"))
                            .on_hover_text("提高排序优先级")
                            .clicked()
                        {
                            move_sort = Some(index);
                        }
                        if ui.button("×").on_hover_text("移除排序").clicked() {
                            remove = Some(index);
                        }
                    });
                }
            } else {
                for (index, filter) in draft.filters.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        field(ui, index, &mut filter.column, definition);
                        egui::ComboBox::from_id_salt(("operator", index))
                            .selected_text(label(filter.condition.operator))
                            .width(65.)
                            .show_ui(ui, |ui| {
                                for operator in [
                                    Operator::Equal,
                                    Operator::NotEqual,
                                    Operator::Like,
                                    Operator::NotLike,
                                    Operator::Less,
                                    Operator::Greater,
                                ] {
                                    ui.selectable_value(
                                        &mut filter.condition.operator,
                                        operator,
                                        label(operator),
                                    );
                                }
                            });
                        if !matches!(
                            filter.condition.operator,
                            Operator::Equal | Operator::NotEqual
                        ) && filter.condition.expected.is_none()
                        {
                            filter.condition.expected = Some(String::new());
                        }
                        let mut null = filter.condition.expected.is_none();
                        if ui
                            .add_enabled(
                                matches!(
                                    filter.condition.operator,
                                    Operator::Equal | Operator::NotEqual
                                ),
                                egui::Checkbox::new(&mut null, "NULL"),
                            )
                            .changed()
                        {
                            filter.condition.expected =
                                if null { None } else { Some(String::new()) };
                        }
                        if let Some(value) = &mut filter.condition.expected {
                            ui.add(TextEdit::singleline(value).desired_width(105.));
                        }
                        if ui.button("×").on_hover_text("移除筛选").clicked() {
                            remove = Some(index);
                        }
                    });
                }
            }
        });
    if let Some(index) = remove {
        if sorting {
            draft.sorts.remove(index);
        } else {
            draft.filters.remove(index);
        }
    }
    if let Some(index) = move_sort {
        draft.sorts.swap(index - 1, index);
    }
    ui.horizontal(|ui| {
        let count = if sorting {
            draft.sorts.len()
        } else {
            draft.filters.len()
        };
        if ui
            .add_enabled(
                count < 64 && !definition.columns.is_empty(),
                egui::Button::new(if sorting {
                    "添加排序"
                } else {
                    "添加条件"
                }),
            )
            .clicked()
        {
            if sorting {
                if let Some(column) = definition
                    .columns
                    .iter()
                    .find(|c| !draft.sorts.iter().any(|s| s.column == c.name))
                {
                    draft.sorts.push(Sort {
                        column: column.name.clone(),
                        descending: false,
                    });
                }
            } else {
                draft.filters.push(ColumnFilter {
                    column: definition.columns[0].name.clone(),
                    condition: Condition {
                        operator: Operator::Equal,
                        expected: Some(String::new()),
                    },
                });
            }
        }
        if ui
            .button(if sorting {
                "移除全部排序"
            } else {
                "移除全部筛选"
            })
            .clicked()
        {
            if sorting {
                draft.sorts.clear();
            } else {
                draft.filters.clear();
                draft.search.clear();
            }
            apply = true;
        }
    });
    ui.separator();
    let mut cancel = false;
    ui.horizontal(|ui| {
        apply |= ui.button("应用筛选和排序").clicked();
        cancel = ui.button("取消").clicked();
    });
    if apply {
        // Keep rejected conditions available for correction. Success clears the draft.
        let query = state
            .table_view
            .query_draft
            .as_ref()
            .expect("query draft")
            .clone();
        state.queue_query(query);
        ui.close();
    } else if cancel {
        state.table_view.query_draft = None;
        ui.close();
    }
}
fn field(ui: &mut Ui, index: usize, value: &mut String, definition: &TableDefinition) {
    egui::ComboBox::from_id_salt(("field", index))
        .selected_text(value.as_str())
        .width(120.)
        .show_ui(ui, |ui| {
            for column in &definition.columns {
                ui.selectable_value(value, column.name.clone(), &column.name);
            }
        });
}
fn label(operator: Operator) -> &'static str {
    match operator {
        Operator::Equal => "=",
        Operator::NotEqual => "≠",
        Operator::Like => "包含",
        Operator::NotLike => "不包含",
        Operator::Less => "<",
        Operator::Greater => ">",
    }
}
