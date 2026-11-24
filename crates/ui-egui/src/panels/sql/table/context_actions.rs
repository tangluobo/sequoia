//! Menu commands run after painting so selection/metadata borrows cannot outlive a frame.
use super::*;
use context_menu::Action;
use tomato_core::result_filter::Condition;
pub(super) fn apply(
    ctx: &egui::Context,
    state: &mut Workbench,
    action: Action,
    id: egui::Id,
    database: &str,
    writable: bool,
) {
    if !editor::finish(ctx, state, database, editor::End::Commit) {
        return;
    }
    match action {
        Action::Set(value) if writable => {
            let cells: Vec<_> = state.table_view.cells.iter().collect();
            for (row, column) in cells {
                if editable(state, row, column) {
                    set_value(state, row, column, Some(value.clone()));
                }
            }
        }
        Action::Delete if writable => state.confirm_delete = true,
        Action::Copy(kind) => match transfer::formatted(state, kind, false) {
            Ok(text) => ctx.copy_text(text),
            Err(error) => state.status = error.to_string(),
        },
        Action::Paste if writable => {
            state.table_view.pending_paste = true;
            ctx.memory_mut(|memory| memory.request_focus(id));
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        Action::Export => state.table_view.export_requested = true,
        Action::Sort(column, descending, append) => {
            if let Some(name) = column_name(state, column) {
                let mut query = state.query.clone();
                if !append {
                    query.sorts.clear();
                }
                query.sorts.retain(|sort| sort.column != name);
                query.sorts.push(Sort {
                    column: name,
                    descending,
                });
                state.queue_query(query);
            }
        }
        Action::RemoveSort(column) => {
            let mut query = state.query.clone();
            if let Some(name) = column_name(state, column) {
                query.sorts.retain(|sort| sort.column != name);
            }
            state.queue_query(query);
        }
        Action::Filter(column, operator) => {
            if let Some((row, _)) = state.table_view.focus {
                let expected = projection::text(state, row, column).map(|value| value.into_owned());
                queue_filter(state, column, Condition { operator, expected });
            }
        }
        Action::CustomFilter(column) => {
            let input = state
                .table_view
                .focus
                .and_then(|(row, _)| projection::text(state, row, column))
                .unwrap_or_default()
                .into_owned();
            state.table_view.projection.dialog = Some(crate::tabs::query_results::view::Dialog {
                column,
                input,
                editor: Default::default(),
                focus: true,
            });
            state.table_view.projection.dialog_context = Some((ctx.clone(), id));
        }
        Action::RemoveFilter(column) => {
            let mut query = state.query.clone();
            if let Some(name) = column_name(state, column) {
                query.filters.retain(|filter| filter.column != name);
            }
            state.queue_query(query);
        }
        Action::ClearFilters => {
            let mut query = state.query.clone();
            query.filters.clear();
            query.search.clear();
            state.queue_query(query);
        }
        Action::Clear => {
            state.queue_query(TableQuery::default());
        }
        Action::Visible(column, visible) => {
            if visible {
                state.table_view.projection.hidden.remove(&column);
            } else {
                state.table_view.projection.hidden.insert(column);
            }
        }
        Action::Refresh => {
            state.pending_page = state.request();
        }
        _ => {}
    }
    ctx.request_repaint();
}
fn column_name(state: &Workbench, column: usize) -> Option<String> {
    state
        .definition
        .as_ref()?
        .columns
        .get(column)
        .map(|column| column.name.clone())
}
fn queue_filter(state: &mut Workbench, column: usize, condition: Condition) {
    if let Some(name) = column_name(state, column) {
        let mut query = state.query.clone();
        query.filters.retain(|filter| filter.column != name);
        query.filters.push(ColumnFilter {
            column: name,
            condition,
        });
        state.queue_query(query);
    }
}
fn editable(state: &Workbench, row: usize, column: usize) -> bool {
    !state.deleted.contains(&row)
        && state
            .page
            .as_ref()
            .and_then(|page| page.definition.columns.get(column))
            .is_some_and(|column| !column.generated)
}
pub(super) fn paste(state: &mut Workbench, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(text.len() <= 8 * 1024 * 1024, "粘贴内容超过 8 MiB");
    if text.is_empty() {
        return Ok(());
    }
    let Some(page) = &state.page else {
        return Ok(());
    };
    let width = page.definition.columns.len();
    let loaded = page.rows.len();
    if width == 0 {
        return Ok(());
    }
    let lines: Vec<_> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let length = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(1, |index| index + 1);
    let rows = projection::rows(state);
    let mut order = projection::order(state);
    let selection: Vec<_> = state.table_view.cells.iter().collect();
    let mut start: Option<(usize, usize)> = None;
    for &(row, column) in &selection {
        if let Some(position) = rows.iter().position(|value| *value == row) {
            start = Some(start.map_or((position, column), |(a, b)| {
                (a.min(position), b.min(column))
            }));
        }
    }
    let start = start.or_else(|| {
        state.table_view.focus.and_then(|(row, column)| {
            rows.iter()
                .position(|value| *value == row)
                .map(|row| (row, column))
        })
    });
    let row_mode = state.table_view.row_mode || start.is_none();
    let (start_row, start_col) = start.unwrap_or((rows.len(), 0));
    let extra = if row_mode {
        length
    } else {
        (start_row + length).saturating_sub(rows.len())
    };
    anyhow::ensure!(
        state.inserted.len() + extra <= 1000,
        "新增行超过 1000 行，请分批粘贴"
    );
    let cells = if row_mode {
        width.saturating_mul(length)
    } else {
        lines
            .iter()
            .take(length)
            .map(|line| {
                line.split('\t')
                    .count()
                    .min(width.saturating_sub(start_col))
            })
            .sum::<usize>()
    };
    let retained = width
        .saturating_mul(extra)
        .saturating_mul(std::mem::size_of::<Option<Cell>>())
        .saturating_add(cells.saturating_mul(std::mem::size_of::<(usize, usize, Cell)>()))
        .saturating_add(text.len());
    anyhow::ensure!(
        retained <= 16 * 1024 * 1024,
        "粘贴暂存数据超过 16 MiB，请分批粘贴"
    );
    let mut staged = Vec::new();
    let mut inserted = Vec::new();
    for (offset, line) in lines.into_iter().take(length).enumerate() {
        let row = if row_mode {
            loaded + state.inserted.len() + offset
        } else {
            rows.get(start_row + offset)
                .copied()
                .unwrap_or(loaded + state.inserted.len() + inserted.len())
        };
        if row_mode || row >= loaded + state.inserted.len() {
            inserted.push(vec![None; width]);
        }
        for (column, value) in line.split('\t').enumerate() {
            let column = column + if row_mode { 0 } else { start_col };
            if column >= width {
                break;
            }
            if !page.definition.columns[column].generated && !state.deleted.contains(&row) {
                staged.push((row, column, Cell::Text(value.into())));
            }
        }
        if row_mode {
            for column in line.split('\t').count().min(width)..width {
                if !page.definition.columns[column].generated {
                    staged.push((row, column, Cell::Text(String::new())));
                }
            }
        }
    }
    let first_new = loaded + state.inserted.len();
    state.inserted.extend(inserted);
    if row_mode {
        let position = state
            .table_view
            .focus
            .and_then(|(row, _)| order.iter().position(|value| *value == row))
            .map_or(order.len(), |index| index + 1);
        order.splice(position..position, first_new..first_new + extra);
    } else {
        order.extend(first_new..first_new + extra);
    }
    state.table_view.projection.order = order;
    for (row, column, value) in staged {
        set_value(state, row, column, Some(value));
    }
    if row_mode {
        state.table_view.cells.clear();
        state.selected.clear();
        state.table_view.selector_rows.clear();
        state.table_view.selector_rows.insert(first_new);
        state.table_view.selector_focus = true;
        state.table_view.row_mode = true;
        state.table_view.row_anchor = Some(first_new);
        state.selected.insert(first_new);
        for column in 0..width {
            state.table_view.cells.insert((first_new, column));
        }
        state.table_view.focus = Some((first_new, 0));
        state.table_view.anchor = state.table_view.focus;
    }
    Ok(())
}
pub(super) fn dialog(ctx: &egui::Context, state: &mut Workbench) {
    let Some(mut dialog) = state.table_view.projection.dialog.take() else {
        return;
    };
    let id = state
        .table_view
        .projection
        .dialog_context
        .as_ref()
        .map_or(egui::Id::new("open-table-filter"), |entry| entry.1);
    let (accept, close) = super::super::results::filter_dialog::show(ctx, &mut dialog, id);
    if accept {
        queue_filter(state, dialog.column, Condition::parse(Some(&dialog.input)));
    }
    if accept || close {
        dialog.editor.release(ctx);
        state.table_view.projection.dialog_context = None;
    } else {
        state.table_view.projection.dialog = Some(dialog);
    }
}
