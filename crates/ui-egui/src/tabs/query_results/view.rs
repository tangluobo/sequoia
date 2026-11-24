//! Projection over retained rows/columns. Transformations never clone cell values.
use super::*;
use std::{collections::BTreeMap, sync::Arc};
use tomato_core::result_filter::Condition;
#[derive(Clone)]
pub struct Column {
    pub identity: usize,
    pub visible: bool,
}
#[derive(Default)]
pub struct View {
    pub source: Option<Vec<usize>>,
    pub filters: BTreeMap<usize, Condition>,
    pub sorts: Vec<(usize, bool)>,
    pub columns: Vec<Column>,
    pub visible: Arc<Vec<usize>>,
    pub dialog: Option<Dialog>,
    pub dragging: Option<Drag>,
}
pub struct Dialog {
    pub focus: bool,
    pub column: usize,
    pub input: String,
    pub editor: crate::tabs::text_state::Editor,
}
pub struct Drag {
    pub column: usize,
    pub target: Option<(usize, bool)>,
}
impl View {
    pub fn initialize(&mut self, columns: usize) {
        if self.columns.len() != columns {
            self.columns = (0..columns)
                .map(|identity| Column {
                    identity,
                    visible: true,
                })
                .collect();
            self.refresh_columns();
        }
    }
    fn refresh_columns(&mut self) {
        self.visible = Arc::new(
            self.columns
                .iter()
                .filter(|column| column.visible)
                .map(|column| column.identity)
                .collect(),
        );
    }
}
impl State {
    pub fn visible_rows(&self) -> usize {
        self.rows
    }
    pub fn visible_columns(&self) -> Arc<Vec<usize>> {
        self.view.visible.clone()
    }
    pub fn column_neighbor(&self, column: usize, direction: i32) -> Option<usize> {
        let columns = &self.view.visible;
        let index = columns.iter().position(|candidate| *candidate == column)?;
        let next = if direction < 0 {
            index.checked_sub(1)?
        } else {
            index.checked_add(1)?
        };
        columns.get(next).copied()
    }
    fn physical_focus(&self) -> (Option<Cell>, Option<Cell>) {
        (
            self.anchor.map(|cell| Cell {
                row: self.row(cell.row),
                ..cell
            }),
            self.focus.map(|cell| Cell {
                row: self.row(cell.row),
                ..cell
            }),
        )
    }
    fn restore_focus(&mut self, (anchor, focus): (Option<Cell>, Option<Cell>)) {
        let position = |cell: Cell| {
            if self.order.is_empty() && self.view.source.is_none() {
                (cell.row < self.rows).then_some(cell)
            } else {
                self.order
                    .iter()
                    .position(|row| *row == cell.row)
                    .map(|row| Cell { row, ..cell })
            }
        };
        self.anchor = anchor.and_then(position);
        self.focus = focus.and_then(position);
        self.drag = None;
    }
    fn sort_projection(&mut self, result: &QueryResult) {
        let editing = &self.editing;
        let sorts = &self.view.sorts;
        let value = |row, column| {
            editing.as_ref().map_or_else(
                || result.value(row, column),
                |editing| editing.value(result, row, column),
            )
        };
        self.order.sort_by(|&left, &right| {
            for (column, descending) in sorts {
                let compared = match (value(left, *column), value(right, *column)) {
                    (Some(a), Some(b)) => a.encode_utf16().cmp(b.encode_utf16()),
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (Some(_), None) => std::cmp::Ordering::Greater,
                };
                if !compared.is_eq() {
                    return if *descending {
                        compared.reverse()
                    } else {
                        compared
                    };
                }
            }
            std::cmp::Ordering::Equal
        });
    }
    fn retain_visible_selection(&mut self) {
        if self.selected.is_empty() && self.row_flags.is_empty() {
            return;
        }
        let mut visible = vec![false; self.physical_rows];
        for row in &self.order {
            if let Some(visible) = visible.get_mut(*row) {
                *visible = true;
            }
        }
        for (row, visible) in visible.into_iter().enumerate() {
            if !visible {
                if let Some(bits) = self.row_flags.get_mut(row / 64) {
                    *bits &= !(1 << (row % 64));
                }
                for column in 0..self.columns {
                    let index = row * self.columns + column;
                    if let Some(bits) = self.selected.get_mut(index / 64) {
                        *bits &= !(1 << (index % 64));
                    }
                }
            }
        }
    }
    fn rebuild_projection(&mut self, result: &QueryResult) {
        if let Some(source) = &self.view.source {
            self.order = source
                .iter()
                .copied()
                .filter(|row| {
                    self.view.filters.iter().all(|(column, filter)| {
                        filter.matches(self.display_value(result, *row, *column))
                    })
                })
                .collect();
        } else if self.order.len() != self.physical_rows {
            self.order = (0..self.physical_rows).collect();
        }
        if !self.view.sorts.is_empty() {
            self.sort_projection(result);
        }
        self.rows = self.order.len();
        self.retain_visible_selection();
    }
    pub fn set_filter(
        &mut self,
        result: &QueryResult,
        column: usize,
        condition: Condition,
    ) -> anyhow::Result<()> {
        self.initialize(result);
        anyhow::ensure!(column < self.columns, "筛选字段已变化");
        let bytes = self
            .view
            .filters
            .iter()
            .filter(|(key, _)| **key != column)
            .fold(
                128 + condition.expected.as_ref().map_or(0, String::capacity),
                |bytes, (_, condition)| {
                    bytes.saturating_add(
                        128 + condition.expected.as_ref().map_or(0, String::capacity),
                    )
                },
            );
        anyhow::ensure!(bytes <= 1024 * 1024, "筛选条件超过 1 MiB");
        let focus = self.physical_focus();
        if self.view.source.is_none() {
            self.view.source = Some(if self.order.is_empty() {
                (0..self.physical_rows).collect()
            } else {
                self.order.clone()
            });
        }
        self.view.filters.insert(column, condition);
        self.rebuild_projection(result);
        self.restore_focus(focus);
        Ok(())
    }
    pub fn remove_filter(&mut self, result: &QueryResult, column: usize) {
        let focus = self.physical_focus();
        self.view.filters.remove(&column);
        if self.view.filters.is_empty() {
            self.clear_filters();
            return;
        }
        self.rebuild_projection(result);
        self.restore_focus(focus);
    }
    pub fn clear_filters(&mut self) {
        let focus = self.physical_focus();
        self.view.filters.clear();
        if let Some(source) = self.view.source.take() {
            self.order = source;
            self.view.sorts.clear();
            self.sort = None;
        }
        self.rows = self.physical_rows;
        self.restore_focus(focus);
    }
    pub fn set_sort(
        &mut self,
        result: &QueryResult,
        column: usize,
        descending: bool,
        additive: bool,
    ) {
        self.initialize(result);
        let focus = self.physical_focus();
        if !additive {
            self.view.sorts.clear();
        }
        if let Some(sort) = self.view.sorts.iter_mut().find(|sort| sort.0 == column) {
            sort.1 = descending;
        } else {
            self.view.sorts.push((column, descending));
        }
        self.sort = self.view.sorts.first().copied();
        if self.order.len() != self.physical_rows && self.view.source.is_none() {
            self.order = (0..self.physical_rows).collect();
        }
        if self.view.sorts.is_empty() {
            self.rebuild_projection(result);
        } else {
            self.sort_projection(result);
            self.rows = self.order.len();
        }
        self.restore_focus(focus);
    }
    pub fn remove_sort(&mut self, result: &QueryResult, column: Option<usize>) {
        let focus = self.physical_focus();
        if let Some(column) = column {
            self.view.sorts.retain(|sort| sort.0 != column);
        } else {
            self.view.sorts.clear();
        }
        self.sort = self.view.sorts.first().copied();
        if self.order.len() != self.physical_rows && self.view.source.is_none() {
            self.order = (0..self.physical_rows).collect();
        }
        if self.view.sorts.is_empty() {
            self.rebuild_projection(result);
        } else {
            self.sort_projection(result);
            self.rows = self.order.len();
        }
        self.restore_focus(focus);
    }
    pub fn cycle_sort(&mut self, result: &QueryResult, column: usize, additive: bool) {
        match self
            .view
            .sorts
            .iter()
            .find(|sort| sort.0 == column)
            .map(|sort| sort.1)
        {
            Some(false) => self.set_sort(result, column, true, additive),
            Some(true) => {
                if additive {
                    self.remove_sort(result, Some(column));
                } else {
                    self.remove_sort(result, None);
                }
            }
            None => self.set_sort(result, column, false, additive),
        }
    }
    pub fn set_column_visible(&mut self, column: usize, visible: bool) {
        if let Some(column) = self
            .view
            .columns
            .iter_mut()
            .find(|entry| entry.identity == column)
        {
            column.visible = visible;
            self.view.refresh_columns();
        }
    }
    pub fn reorder_columns(&mut self, order: Vec<usize>) -> anyhow::Result<()> {
        anyhow::ensure!(order.len() == self.columns, "列顺序不完整");
        let mut seen = vec![false; self.columns];
        for column in &order {
            anyhow::ensure!(
                *column < self.columns && !seen[*column],
                "列顺序包含重复或未知字段"
            );
            seen[*column] = true;
        }
        let visible: Vec<_> = (0..self.columns)
            .map(|identity| {
                self.view
                    .columns
                    .iter()
                    .find(|column| column.identity == identity)
                    .is_some_and(|column| column.visible)
            })
            .collect();
        self.view.columns = order
            .into_iter()
            .map(|identity| Column {
                identity,
                visible: visible[identity],
            })
            .collect();
        self.view.refresh_columns();
        Ok(())
    }
    pub fn move_column(&mut self, source: usize, target: usize, after: bool) -> anyhow::Result<()> {
        if source == target {
            return Ok(());
        }
        let mut order: Vec<_> = self
            .view
            .columns
            .iter()
            .map(|column| column.identity)
            .filter(|column| *column != source)
            .collect();
        let Some(index) = order.iter().position(|column| *column == target) else {
            return Ok(());
        };
        order.insert(index + usize::from(after), source);
        self.reorder_columns(order)
    }
    pub fn committed(&mut self, result: &mut QueryResult, deleting: bool) {
        let removed = self
            .editing
            .as_ref()
            .filter(|_| deleting)
            .map(|editing| editing.deleted.clone())
            .unwrap_or_default();
        if let Some(editing) = &mut self.editing {
            editing.committed(result, deleting);
        }
        if deleting {
            let remap = |row| {
                if removed.contains(&row) {
                    None
                } else {
                    Some(row - removed.range(..row).count())
                }
            };
            self.order = std::mem::take(&mut self.order)
                .into_iter()
                .filter_map(remap)
                .collect();
            if let Some(source) = self.view.source.take() {
                self.view.source = Some(source.into_iter().filter_map(remap).collect());
            }
            self.physical_rows = result.rows.len();
            self.rows = if self.view.source.is_some() {
                self.order.len()
            } else {
                self.physical_rows
            };
            self.reset_selection();
        }
    }
}

impl State {
    pub fn release(&self, ctx: &egui::Context) {
        if let Some(editing) = &self.editing {
            editing.release(ctx);
        }
        if let Some(dialog) = &self.view.dialog {
            dialog.editor.release(ctx);
        }
    }
}
