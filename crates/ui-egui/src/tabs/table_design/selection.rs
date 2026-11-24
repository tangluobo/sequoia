//! Java field-grid selection, stored as eight visible-column bits per row.
use super::{Cell, Draft};

#[derive(Clone, Copy)]
pub enum Drag {
    Rows { start: usize, moved: bool },
    Cells { start: Cell, moved: bool },
}
impl Drag {
    pub fn moved(self) -> bool {
        match self {
            Self::Rows { moved, .. } | Self::Cells { moved, .. } => moved,
        }
    }
    pub fn rows(self) -> bool {
        matches!(self, Self::Rows { .. })
    }
}
impl Draft {
    pub fn selected(&self, row: usize, column: usize) -> bool {
        column < 8
            && self
                .selected
                .get(row)
                .is_some_and(|mask| mask & (1 << column) != 0)
    }
    pub fn row_selected(&self, row: usize) -> bool {
        self.selected.get(row).is_some_and(|mask| *mask != 0)
    }
    pub fn selected_rows(&self) -> Vec<usize> {
        self.selected
            .iter()
            .enumerate()
            .filter_map(|(row, mask)| (*mask != 0).then_some(row))
            .collect()
    }
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.focus = None;
        self.anchor = None;
        self.drag = None;
    }
    pub fn select(&mut self, cell: Cell, control: bool, shift: bool) {
        if cell.row >= self.request.columns.len() || cell.column >= 8 {
            return;
        }
        if !control {
            self.selected.fill(0);
        }
        self.selected.resize(self.request.columns.len(), 0);
        if shift && let Some(anchor) = self.anchor {
            for row in anchor.row.min(cell.row)..=anchor.row.max(cell.row) {
                for column in anchor.column.min(cell.column)..=anchor.column.max(cell.column) {
                    self.selected[row] |= 1 << column;
                }
            }
        } else {
            if control {
                self.selected[cell.row] ^= 1 << cell.column;
            } else {
                self.selected[cell.row] |= 1 << cell.column;
            }
            self.anchor = Some(cell);
        }
        self.focus = Some(cell);
    }
    pub fn select_row(&mut self, row: usize) {
        self.select_rows(row, false, false);
    }
    pub fn select_rows(&mut self, row: usize, control: bool, shift: bool) {
        if row >= self.request.columns.len() {
            return;
        }
        self.drag = None;
        self.selected.resize(self.request.columns.len(), 0);
        // The Java row selector gives Ctrl precedence, including Ctrl+Shift.
        if control {
            if self.row_selected(row) {
                self.selected[row] = 0;
            } else {
                self.selected[row] = 0xff;
                self.focus = Some(Cell { row, column: 7 });
            }
            return;
        }
        let start = if shift {
            self.focus.map_or(row, |cell| cell.row)
        } else {
            row
        };
        self.selected.fill(0);
        self.selected[start.min(row)..=start.max(row)].fill(0xff);
        self.anchor = None;
        self.focus = Some(Cell { row, column: 7 });
    }
    pub fn begin_selection(&mut self, cell: Cell, whole_row: bool, control: bool, shift: bool) {
        self.drag = None;
        if cell.row >= self.request.columns.len() || cell.column >= 8 {
            return;
        }
        if whole_row {
            self.select_rows(cell.row, control, shift);
        } else {
            self.select(cell, control, shift);
        }
        if !control && !shift {
            self.drag = Some(if whole_row {
                Drag::Rows {
                    start: cell.row,
                    moved: false,
                }
            } else {
                Drag::Cells {
                    start: cell,
                    moved: false,
                }
            });
        }
    }
    pub fn drag_to(&mut self, cell: Cell) {
        if cell.row >= self.request.columns.len() || cell.column >= 8 {
            return;
        }
        match self.drag {
            Some(Drag::Rows { start, moved }) => {
                self.selected.fill(0);
                self.selected[start.min(cell.row)..=start.max(cell.row)].fill(0xff);
                self.focus = Some(Cell {
                    row: cell.row,
                    column: 7,
                });
                self.drag = Some(Drag::Rows {
                    start,
                    moved: moved || cell.row != start,
                });
            }
            Some(Drag::Cells { start, moved }) => {
                self.anchor = Some(start);
                self.select(cell, false, true);
                self.drag = Some(Drag::Cells {
                    start,
                    moved: moved || cell != start,
                });
            }
            None => {}
        }
    }
    pub fn select_column(&mut self, column: usize) {
        if column >= 8 {
            return;
        }
        self.clear_selection();
        self.selected
            .resize(self.request.columns.len(), 1 << column);
        self.focus = self
            .request
            .columns
            .len()
            .checked_sub(1)
            .map(|row| Cell { row, column });
    }
    pub fn select_all(&mut self) {
        self.drag = None;
        self.selected.resize(self.request.columns.len(), 0xff);
        self.selected.fill(0xff);
        self.focus = self
            .request
            .columns
            .len()
            .checked_sub(1)
            .map(|row| Cell { row, column: 7 });
    }
}
