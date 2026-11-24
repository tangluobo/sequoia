//! Preview selected cells in one buffer; validate the entire write set before changing drafts.
use super::*;
use crate::panels::sql::batch_input::Target;
use crate::tabs::batch_edit::{Batch, Edit};
pub(super) struct Input<'a> {
    pub state: &'a mut Workbench,
    pub database: &'a str,
}
impl Target for Input<'_> {
    fn active(&self) -> bool {
        self.state.batch.is_some()
    }
    fn composing(&self) -> bool {
        self.state
            .batch
            .as_ref()
            .is_some_and(|batch| batch.composing)
    }
    fn start(&mut self) -> anyhow::Result<bool> {
        let Some(page) = &self.state.page else {
            return Ok(false);
        };
        let cells = self.state.table_view.cells.iter().filter(|(row, column)| {
            !self.state.deleted.contains(row)
                && !self.state.table_view.projection.hidden.contains(column)
                && *row < page.rows.len() + self.state.inserted.len()
                && page
                    .definition
                    .columns
                    .get(*column)
                    .is_some_and(|column| !column.generated)
        });
        self.state.batch = Batch::new(cells)?.map(Box::new);
        Ok(self.active())
    }
    fn edit(&mut self, edit: Edit<'_>) -> anyhow::Result<()> {
        self.state.batch.as_mut().unwrap().edit(edit)
    }
    fn finish(&mut self) -> anyhow::Result<()> {
        finish(self.state, self.database)
    }
    fn cancel(&mut self) {
        self.state.batch = None;
        self.state.status.clear();
    }
    fn error(&mut self, error: anyhow::Error) {
        self.state.status = format!("批量输入无效：{error}");
    }
}
pub(super) fn finish(state: &mut Workbench, database: &str) -> anyhow::Result<()> {
    let Some(batch) = &state.batch else {
        return Ok(());
    };
    anyhow::ensure!(!batch.composing, "请先完成输入法组词");
    let Some(page) = &state.page else {
        state.batch = None;
        return Ok(());
    };
    anyhow::ensure!(
        !page.definition.view
            && page.definition.transactional
            && !state.needs_refresh
            && !state.session_lost,
        "当前结果不可编辑"
    );
    let mut staged = Vec::with_capacity(batch.cells.len());
    let mut bytes = 0usize;
    let mut cloned_rows = std::collections::BTreeSet::new();
    for &(row, column) in &batch.cells {
        anyhow::ensure!(!state.deleted.contains(&row), "选中的行已删除");
        let field = page
            .definition
            .columns
            .get(column)
            .ok_or_else(|| anyhow::anyhow!("字段已变化"))?;
        anyhow::ensure!(!field.generated, "生成字段不可编辑");
        let value = editor::parse(state, row, column, &batch.text, field.binary, database)?;
        bytes = bytes.saturating_add(match &value {
            Cell::Text(text) => text.len(),
            Cell::Binary(bytes) => bytes.len(),
            _ => 32,
        });
        if row < page.rows.len() && !state.edits.contains_key(&row) && cloned_rows.insert(row) {
            for cell in &page.rows[row] {
                bytes = bytes.saturating_add(match cell {
                    Cell::Text(text) => text.len(),
                    Cell::Binary(bytes) => bytes.len(),
                    _ => 32,
                });
            }
        }
        anyhow::ensure!(bytes <= 16 * 1024 * 1024, "批量编辑暂存数据超过 16 MiB");
        staged.push((row, column, value));
    }
    state.batch = None;
    for (row, column, value) in staged {
        editor::set_value(state, row, column, Some(value));
    }
    state.status.clear();
    Ok(())
}
