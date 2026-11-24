//! Sparse pending edits use physical row identities, independently of display sorting.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod batch;
#[cfg(test)]
mod tests;
pub struct Editing {
    pub editor_state: crate::tabs::text_state::Editor,
    pub target: tomato_services::database::query_rows::Editing,
    pub cells: BTreeMap<(usize, usize), Option<String>>,
    pub deleted: BTreeSet<usize>,
    pub editor: Option<Editor>,
    pub batch: Option<Box<crate::tabs::batch_edit::Batch>>,
    pub enabled: bool,
    pub confirm_delete: bool,
}
pub struct Editor {
    pub temporal: Option<Box<super::temporal::Picker>>,
    pub select_all: bool,
    pub changed: bool,
    pub row: usize,
    pub column: usize,
    pub text: String,
    pub null: bool,
    pub focus: bool,
}
impl Editing {
    pub fn new(target: tomato_services::database::query_rows::Editing) -> Self {
        Self {
            editor_state: Default::default(),
            target,
            cells: BTreeMap::new(),
            deleted: BTreeSet::new(),
            editor: None,
            batch: None,
            enabled: true,
            confirm_delete: false,
        }
    }
    pub fn dirty(&self) -> bool {
        !self.cells.is_empty()
            || !self.deleted.is_empty()
            || self.editor.as_ref().is_some_and(|editor| editor.changed)
            || self.batch.is_some()
    }
    pub fn writable(&self, column: usize) -> bool {
        self.enabled && self.target.plan.editable_column_indexes.contains(&column)
    }
    pub fn value<'a>(
        &'a self,
        result: &'a QueryResult,
        row: usize,
        column: usize,
    ) -> Option<&'a str> {
        if let Some(value) = self
            .batch
            .as_ref()
            .and_then(|batch| batch.value(row, column))
        {
            return Some(value);
        }
        self.cells
            .get(&(row, column))
            .map_or_else(|| result.value(row, column), |value| value.as_deref())
    }
    fn validate(
        &self,
        result: &QueryResult,
        row: usize,
        column: usize,
        value: Option<&str>,
        capacity: usize,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            row < result.rows.len() && self.writable(column),
            "当前单元格不可编辑"
        );
        if result.value(row, column) == value {
            return Ok(());
        }
        let current = self
            .cells
            .values()
            .fold(self.cells.len() * 128, |bytes, value| {
                bytes.saturating_add(value.as_ref().map_or(0, String::capacity))
            });
        let replaced = self
            .cells
            .get(&(row, column))
            .map_or(0, |value| 128 + value.as_ref().map_or(0, String::capacity));
        anyhow::ensure!(
            current
                .saturating_sub(replaced)
                .saturating_add(128 + capacity)
                <= 8 * 1024 * 1024,
            "待保存内容超过 8 MiB"
        );
        Ok(())
    }
    pub fn set(
        &mut self,
        result: &QueryResult,
        row: usize,
        column: usize,
        value: Option<String>,
    ) -> anyhow::Result<()> {
        self.validate(
            result,
            row,
            column,
            value.as_deref(),
            value.as_ref().map_or(0, String::capacity),
        )?;
        if result.value(row, column) == value.as_deref() {
            self.cells.remove(&(row, column));
        } else {
            self.cells.insert((row, column), value);
        }
        Ok(())
    }
    pub fn release(&self, ctx: &egui::Context) {
        self.editor_state.release(ctx);
        if let Some(picker) = self
            .editor
            .as_ref()
            .and_then(|editor| editor.temporal.as_ref())
        {
            picker.release(ctx);
        }
    }
    pub fn open(&mut self, result: &QueryResult, row: usize, column: usize) {
        if !self.writable(column) || self.deleted.contains(&row) {
            return;
        }
        self.editor_state.reset();
        let value = self.value(result, row, column);
        self.editor = Some(Editor {
            temporal: None,
            select_all: true,
            changed: false,
            row,
            column,
            text: value.unwrap_or_default().into(),
            null: value.is_none(),
            focus: true,
        });
    }
    pub fn finish(&mut self, result: &QueryResult) -> anyhow::Result<()> {
        self.finish_batch(result)?;
        if let Some(editor) = &self.editor {
            self.validate(
                result,
                editor.row,
                editor.column,
                if editor.null {
                    None
                } else {
                    Some(&editor.text)
                },
                if editor.null {
                    0
                } else {
                    editor.text.capacity()
                },
            )?;
        }
        if let Some(editor) = self.editor.take() {
            let value = if editor.null { None } else { Some(editor.text) };
            if result.value(editor.row, editor.column) == value.as_deref() {
                self.cells.remove(&(editor.row, editor.column));
            } else {
                self.cells.insert((editor.row, editor.column), value);
            }
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.batch = None;
        self.cells.clear();
        self.deleted.clear();
        self.editor = None;
        self.confirm_delete = false;
    }
    pub fn changes(
        &self,
        result: &QueryResult,
        deleting: bool,
    ) -> anyhow::Result<Vec<tomato_services::database::query_rows::Change>> {
        let rows: BTreeSet<_> = if deleting {
            self.deleted.clone()
        } else {
            self.cells.keys().map(|(row, _)| *row).collect()
        };
        anyhow::ensure!(rows.len() <= 1000, "每批最多保存 1,000 行修改");
        Ok(rows
            .into_iter()
            .map(|row| tomato_services::database::query_rows::Change {
                original: (0..result.columns.len())
                    .map(|column| result.value(row, column).map(str::to_owned))
                    .collect(),
                modified: if deleting {
                    BTreeMap::new()
                } else {
                    self.cells
                        .range((row, 0)..=(row, usize::MAX))
                        .map(|((_, column), value)| (*column, value.clone()))
                        .collect()
                },
                delete: deleting,
            })
            .collect())
    }
    pub fn committed(&mut self, result: &mut QueryResult, deleting: bool) {
        self.batch = None;
        // Rebuild only the one-bit NULL sidecar when rows are removed. Strings
        // move into the retained rows; neither a result nor a whole row is cloned.
        let old_null = std::mem::take(&mut result.null_cells);
        let width = result.columns.len();
        let mut new_null = Vec::new();
        let mut to = 0usize;
        let mut from = 0usize;
        result.rows.retain_mut(|row| {
            let source = from;
            from += 1;
            if deleting && self.deleted.contains(&source) {
                return false;
            }
            for (column, cell) in row.iter_mut().enumerate() {
                let changed = if deleting {
                    None
                } else {
                    self.cells.remove(&(source, column))
                };
                let null = if let Some(value) = changed {
                    let null = value.is_none();
                    *cell = value.unwrap_or_default();
                    null
                } else {
                    let index = source * width + column;
                    old_null
                        .get(index / 64)
                        .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
                };
                if null {
                    let index = to * width + column;
                    new_null.resize(new_null.len().max(index / 64 + 1), 0);
                    new_null[index / 64] |= 1 << (index % 64);
                }
            }
            to += 1;
            true
        });
        result.null_cells = new_null;
        if deleting {
            result.total_rows = result.total_rows.saturating_sub(self.deleted.len() as u64);
            self.cells = std::mem::take(&mut self.cells)
                .into_iter()
                .filter_map(|((row, column), value)| {
                    if self.deleted.contains(&row) {
                        None
                    } else {
                        Some(((row - self.deleted.range(..row).count(), column), value))
                    }
                })
                .collect();
            self.deleted.clear();
            self.editor = None;
            self.confirm_delete = false;
        } else {
            self.cancel();
        }
    }
}
impl State {
    pub fn reset_selection(&mut self) {
        self.selected.clear();
        self.row_flags.clear();
        self.anchor_row = false;
        self.anchor = None;
        self.focus = None;
        self.drag = None;
    }
    pub fn display_value<'a>(
        &'a self,
        result: &'a QueryResult,
        row: usize,
        column: usize,
    ) -> Option<&'a str> {
        self.editing.as_ref().map_or_else(
            || result.value(row, column),
            |editing| editing.value(result, row, column),
        )
    }
    pub fn start_edit(&mut self, result: &QueryResult, cell: Cell) {
        let row = self.row(cell.row);
        if let Some(editing) = &mut self.editing {
            if let Err(error) = editing.finish(result) {
                self.error = error.to_string();
                return;
            }
            editing.open(result, row, cell.column);
        }
    }
    pub fn mark_delete(&mut self) {
        let rows: BTreeSet<_> = (0..self.rows)
            .filter(|row| self.row_selected(*row))
            .map(|row| self.row(row))
            .collect();
        if let Some(editing) = &mut self.editing
            && editing.enabled
            && editing.target.plan.can_delete()
        {
            editing.deleted.extend(rows);
            editing.confirm_delete = false;
        }
    }
    pub fn paste(&mut self, result: &QueryResult, text: &str) -> anyhow::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let mut start = None;
        let visible = self.visible_columns();
        for row in 0..self.rows {
            for (position, column) in visible.iter().copied().enumerate() {
                if self.contains(Cell { row, column }) {
                    let (a, b) = start.get_or_insert((row, position));
                    *a = (*a).min(row);
                    *b = (*b).min(position);
                }
            }
        }
        let Some((start_row, start_column)) = start else {
            return Ok(());
        };
        // The original skips expression columns and trailing empty clipboard rows.
        let lines: Vec<_> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        let length = lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(1, |index| index + 1);
        let mut staged = Vec::new();
        let mut bytes = 0usize;
        for (line, row) in lines.iter().take(length).enumerate() {
            let display = start_row + line;
            if display >= self.rows {
                break;
            }
            for (offset, value) in row.split('\t').enumerate() {
                let Some(&column) = visible.get(start_column + offset) else {
                    break;
                };
                if !self
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.writable(column))
                {
                    continue;
                }
                bytes = bytes.saturating_add(value.len());
                anyhow::ensure!(bytes <= 8 * 1024 * 1024, "粘贴内容超过 8 MiB");
                staged.push((display, self.row(display), column, value.to_owned()));
            }
        }
        if let Some(editing) = &mut self.editing {
            let mut projected = editing
                .cells
                .values()
                .fold(editing.cells.len() * 128, |bytes, value| {
                    bytes.saturating_add(value.as_ref().map_or(0, String::capacity))
                });
            for (_, row, column, value) in &staged {
                projected = projected.saturating_sub(
                    editing
                        .cells
                        .get(&(*row, *column))
                        .map_or(0, |value| 128 + value.as_ref().map_or(0, String::capacity)),
                );
                if result.value(*row, *column) != Some(value.as_str()) {
                    projected = projected.saturating_add(128 + value.capacity());
                }
            }
            anyhow::ensure!(projected <= 8 * 1024 * 1024, "待保存内容超过 8 MiB");
            for (_, row, column, value) in &mut staged {
                if result.value(*row, *column) == Some(value.as_str()) {
                    editing.cells.remove(&(*row, *column));
                } else {
                    editing
                        .cells
                        .insert((*row, *column), Some(std::mem::take(value)));
                }
            }
        }
        self.selected.fill(0);
        self.row_flags.fill(0);
        for (display, _, column, _) in staged {
            self.set(
                Cell {
                    row: display,
                    column,
                },
                true,
            );
        }
        Ok(())
    }
}
