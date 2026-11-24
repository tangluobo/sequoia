//! Result selection uses one bit per cell; no allocation until the first selection.
use tomato_services::QueryResult;
pub mod editing;
pub mod temporal;
pub mod temporal_model;
pub mod transfer;
pub mod view;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}
#[derive(Default)]
pub struct State {
    pub export_requested: bool,
    pub view: view::View,
    physical_rows: usize,
    row_flags: Vec<u64>,
    anchor_row: bool,
    pub saving: bool,
    pub edit_status: Option<String>,
    pub source_table: Option<String>,
    pub pending_write: Option<(std::path::PathBuf, String)>,
    pub editing: Option<Box<editing::Editing>>,
    pub widths: Vec<f32>,
    pub order: Vec<usize>,
    pub sort: Option<(usize, bool)>,
    selected: Vec<u64>,
    columns: usize,
    rows: usize,
    pub anchor: Option<Cell>,
    pub focus: Option<Cell>,
    drag: Option<(Cell, bool)>,
    pub error: String,
}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod view_tests;
impl State {
    pub fn initialize(&mut self, result: &QueryResult) {
        self.columns = result.columns.len();
        self.physical_rows = result.rows.len();
        self.view.initialize(self.columns);
        self.rows = if self.view.source.is_some() {
            self.order.len()
        } else {
            self.physical_rows
        };
        if self.widths.len() != self.columns {
            self.widths = result
                .columns
                .iter()
                .map(|name| (name.encode_utf16().count() as f32 * 8. + 16.).max(60.))
                .collect();
        }
    }
    pub fn row(&self, index: usize) -> usize {
        self.order.get(index).copied().unwrap_or(index)
    }
    fn index(&self, cell: Cell) -> Option<usize> {
        if cell.row >= self.rows || cell.column >= self.columns {
            return None;
        }
        self.row(cell.row)
            .checked_mul(self.columns)?
            .checked_add(cell.column)
    }
    pub fn contains(&self, cell: Cell) -> bool {
        self.index(cell)
            .and_then(|index| {
                self.selected
                    .get(index / 64)
                    .map(|bits| bits & (1 << (index % 64)) != 0)
            })
            .unwrap_or(false)
    }
    fn set(&mut self, cell: Cell, value: bool) {
        let Some(index) = self.index(cell) else {
            return;
        };
        self.selected
            .resize((self.physical_rows * self.columns).div_ceil(64), 0);
        if value {
            self.selected[index / 64] |= 1 << (index % 64);
        } else {
            self.selected[index / 64] &= !(1 << (index % 64));
        }
    }
    fn range(&mut self, from: Cell, to: Cell, whole_row: bool, selected: bool) {
        let visible = self.visible_columns();
        let from_column = visible
            .iter()
            .position(|column| *column == from.column)
            .unwrap_or(0);
        let to_column = visible
            .iter()
            .position(|column| *column == to.column)
            .unwrap_or(0);
        let columns: Vec<_> = if whole_row {
            if selected {
                visible.to_vec()
            } else {
                (0..self.columns).collect()
            }
        } else {
            visible
                .iter()
                .skip(from_column.min(to_column))
                .take(from_column.abs_diff(to_column) + 1)
                .copied()
                .collect()
        };
        for row in from.row.min(to.row)..=from.row.max(to.row).min(self.rows.saturating_sub(1)) {
            if whole_row {
                self.set_row(row, selected);
            }
            for column in columns.iter().copied() {
                self.set(Cell { row, column }, selected);
            }
        }
    }
    pub fn select(&mut self, cell: Cell, shift: bool, control: bool, whole_row: bool) {
        let shift = shift && !(whole_row && control);
        let whole_row = whole_row || (shift && self.anchor_row);
        let from = if shift {
            self.anchor.unwrap_or(cell)
        } else {
            cell
        };
        let selected = if control && !shift {
            if whole_row {
                !self.row_selected(cell.row)
            } else {
                !self.contains(cell)
            }
        } else {
            true
        };
        if !control {
            self.selected.fill(0);
            self.row_flags.fill(0);
        }
        self.range(from, cell, whole_row, selected);
        if !shift {
            self.anchor = Some(cell);
            self.anchor_row = whole_row;
        }
        self.focus = Some(cell);
        self.drag = (!control).then_some((from, whole_row));
    }
    pub fn drag_to(&mut self, cell: Cell) {
        if let Some((from, whole_row)) = self.drag {
            self.selected.fill(0);
            self.row_flags.fill(0);
            self.range(from, cell, whole_row, true);
            self.focus = Some(cell);
        }
    }
    pub fn end_drag(&mut self) {
        self.drag = None;
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub fn row_drag(&self) -> bool {
        self.drag.is_some_and(|(_, whole_row)| whole_row)
    }
    pub fn select_all(&mut self) {
        self.selected
            .resize((self.physical_rows * self.columns).div_ceil(64), 0);
        self.selected.fill(0);
        self.row_flags.fill(0);
        let columns = self.visible_columns();
        for row in 0..self.rows {
            self.set_row(row, true);
            for column in columns.iter().copied() {
                self.set(Cell { row, column }, true);
            }
        }
        if self.rows > 0 && self.columns > 0 {
            self.anchor = Some(Cell {
                row: 0,
                column: columns.first().copied().unwrap_or(0),
            });
            self.focus = self.anchor;
            self.anchor_row = true;
        }
    }
    fn set_row(&mut self, row: usize, selected: bool) {
        if row >= self.rows {
            return;
        }
        let physical = self.row(row);
        self.row_flags.resize(self.physical_rows.div_ceil(64), 0);
        if selected {
            self.row_flags[physical / 64] |= 1 << (physical % 64);
        } else {
            self.row_flags[physical / 64] &= !(1 << (physical % 64));
        }
    }
    pub fn row_selected(&self, row: usize) -> bool {
        let physical = self.row(row);
        self.row_flags
            .get(physical / 64)
            .is_some_and(|bits| bits & (1 << (physical % 64)) != 0)
            || (0..self.columns).any(|column| self.contains(Cell { row, column }))
    }
    #[cfg(test)]
    pub fn sort_by(&mut self, result: &QueryResult, column: usize) {
        self.initialize(result);
        self.cycle_sort(result, column, false);
    }
    pub fn copy(&self, result: &QueryResult) -> anyhow::Result<Option<String>> {
        let mut bounds = None;
        for row in 0..self.rows {
            for (visual, column) in self.view.columns.iter().enumerate() {
                let column = column.identity;
                if self.contains(Cell { row, column }) {
                    let (min_row, max_row, min_col, max_col) =
                        bounds.get_or_insert((row, row, visual, visual));
                    *min_row = (*min_row).min(row);
                    *max_row = (*max_row).max(row);
                    *min_col = (*min_col).min(visual);
                    *max_col = (*max_col).max(visual);
                }
            }
        }
        let Some((min_row, max_row, min_col, max_col)) = bounds else {
            return Ok(None);
        };
        let mut text = String::new();
        for row in min_row..=max_row {
            let mut first = true;
            for visual in min_col..=max_col {
                let column = self.view.columns[visual].identity;
                if !self.contains(Cell { row, column }) {
                    continue;
                }
                let value = self
                    .display_value(result, self.row(row), column)
                    .unwrap_or("");
                anyhow::ensure!(
                    text.len().saturating_add(value.len()).saturating_add(2) <= 8 * 1024 * 1024,
                    "选中内容超过 8 MiB，请缩小复制范围"
                );
                if !first {
                    text.push('\t');
                }
                first = false;
                text.push_str(value);
            }
            if row < max_row {
                text.push('\n');
            }
        }
        Ok(Some(text))
    }
}
