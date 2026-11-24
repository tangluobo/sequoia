//! View actions refer to physical fields even after header reordering.
use super::*;
use tomato_core::result_filter::{Condition, Operator};
pub(super) fn menu(ui: &mut Ui, result: &QueryResult, state: &mut State, column: Option<usize>) {
    ui.menu_button("排序", |ui| {
        for (label, descending) in [("升序排序", false), ("降序排序", true)] {
            if ui
                .add_enabled(column.is_some(), egui::Button::new(label))
                .clicked()
            {
                state.set_sort(result, column.unwrap(), descending, false);
                ui.close();
            }
        }
        if ui
            .add_enabled(column.is_some(), egui::Button::new("排序"))
            .clicked()
        {
            let column = column.unwrap();
            if !state.view.sorts.iter().any(|sort| sort.0 == column) {
                state.set_sort(result, column, false, true);
            }
            ui.close();
        }
        ui.separator();
        let sorted =
            column.is_some_and(|column| state.view.sorts.iter().any(|sort| sort.0 == column));
        if ui
            .add_enabled(sorted, egui::Button::new("移除排序"))
            .clicked()
        {
            state.remove_sort(result, column);
            ui.close();
        }
    });
    ui.menu_button("筛选", |ui| {
        let focused = column
            .zip(state.focus)
            .filter(|(_, cell)| cell.row < state.visible_rows());
        for (label, operator) in [
            ("字段 = 值", Operator::Equal),
            ("字段 != 值", Operator::NotEqual),
            ("字段类似值", Operator::Like),
            ("字段不类似值", Operator::NotLike),
            ("字段 < 值", Operator::Less),
            ("字段 > 值", Operator::Greater),
        ] {
            if ui
                .add_enabled(focused.is_some(), egui::Button::new(label))
                .clicked()
            {
                let (column, cell) = focused.unwrap();
                let expected = state
                    .display_value(result, state.row(cell.row), column)
                    .map(str::to_owned);
                if let Err(error) =
                    state.set_filter(result, column, Condition { operator, expected })
                {
                    state.error = error.to_string();
                }
                ui.close();
            }
        }
        ui.separator();
        for label in ["自定义筛选", "筛选"] {
            if ui
                .add_enabled(focused.is_some(), egui::Button::new(label))
                .clicked()
            {
                let (column, cell) = focused.unwrap();
                let input = state
                    .display_value(result, state.row(cell.row), column)
                    .unwrap_or_default()
                    .to_owned();
                state.view.dialog = Some(crate::tabs::query_results::view::Dialog {
                    column,
                    input,
                    editor: Default::default(),
                    focus: true,
                });
                ui.close();
            }
        }
        ui.separator();
        if ui
            .add_enabled(
                column.is_some_and(|column| state.view.filters.contains_key(&column)),
                egui::Button::new("移除筛选"),
            )
            .clicked()
        {
            state.remove_filter(result, column.unwrap());
            ui.close();
        }
        if ui
            .add_enabled(
                state.view.source.is_some(),
                egui::Button::new("移除全部筛选"),
            )
            .clicked()
        {
            state.clear_filters();
            ui.close();
        }
    });
    if ui.button("移除全部排序及筛选").clicked() {
        state.remove_sort(result, None);
        state.clear_filters();
        ui.close();
    }
    ui.separator();
    ui.menu_button("显示", |ui| {
        let columns: Vec<_> = state
            .view
            .columns
            .iter()
            .map(|column| (column.identity, column.visible))
            .collect();
        for (column, mut visible) in columns {
            if ui.checkbox(&mut visible, &result.columns[column]).changed() {
                state.set_column_visible(column, visible);
            }
        }
    });
    if ui.button("刷新").clicked() {
        ui.ctx().request_repaint();
        ui.close();
    }
}
pub(super) fn dialog(ctx: &egui::Context, result: &QueryResult, state: &mut State, id: Id) {
    let Some(mut dialog) = state.view.dialog.take() else {
        return;
    };
    let (accept, close) = super::filter_dialog::show(ctx, &mut dialog, id);
    if accept
        && let Err(error) =
            state.set_filter(result, dialog.column, Condition::parse(Some(&dialog.input)))
    {
        state.error = error.to_string();
    }
    if accept || close {
        dialog.editor.release(ctx);
    } else {
        state.view.dialog = Some(dialog);
    }
}
