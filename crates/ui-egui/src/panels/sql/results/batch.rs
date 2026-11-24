use super::*;
use crate::panels::sql::batch_input::Target;
use crate::tabs::batch_edit::{Batch, Edit};
pub(super) struct Input<'a> {
    pub state: &'a mut State,
    pub result: &'a QueryResult,
}
impl Target for Input<'_> {
    fn active(&self) -> bool {
        self.state
            .editing
            .as_ref()
            .is_some_and(|editing| editing.batch.is_some())
    }
    fn composing(&self) -> bool {
        self.state
            .editing
            .as_ref()
            .and_then(|editing| editing.batch.as_ref())
            .is_some_and(|batch| batch.composing)
    }
    fn start(&mut self) -> anyhow::Result<bool> {
        let Some(editing) = &self.state.editing else {
            return Ok(false);
        };
        let columns = self.state.visible_columns();
        let cells = (0..self.state.visible_rows())
            .flat_map(|row| {
                columns
                    .iter()
                    .copied()
                    .map(move |column| Cell { row, column })
            })
            .filter(|cell| {
                self.state.contains(*cell)
                    && editing.writable(cell.column)
                    && !editing.deleted.contains(&self.state.row(cell.row))
            })
            .map(|cell| (self.state.row(cell.row), cell.column));
        let batch = Batch::new(cells)?;
        self.state.editing.as_mut().unwrap().batch = batch.map(Box::new);
        Ok(self.active())
    }
    fn edit(&mut self, edit: Edit<'_>) -> anyhow::Result<()> {
        self.state
            .editing
            .as_mut()
            .unwrap()
            .batch
            .as_mut()
            .unwrap()
            .edit(edit)
    }
    fn finish(&mut self) -> anyhow::Result<()> {
        self.state
            .editing
            .as_mut()
            .unwrap()
            .finish_batch(self.result)?;
        self.state.error.clear();
        Ok(())
    }
    fn cancel(&mut self) {
        self.state.editing.as_mut().unwrap().batch = None;
        self.state.error.clear();
    }
    fn error(&mut self, error: anyhow::Error) {
        self.state.error = format!("批量输入无效：{error}");
    }
}
