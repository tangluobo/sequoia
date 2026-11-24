use super::*;
impl Editing {
    pub fn finish_batch(&mut self, result: &QueryResult) -> anyhow::Result<()> {
        let Some(batch) = &self.batch else {
            return Ok(());
        };
        anyhow::ensure!(!batch.composing, "请先完成输入法组词");
        let mut bytes = self
            .cells
            .values()
            .fold(self.cells.len() * 128, |bytes, value| {
                bytes.saturating_add(value.as_ref().map_or(0, String::capacity))
            });
        for &(row, column) in &batch.cells {
            anyhow::ensure!(
                row < result.rows.len() && self.writable(column) && !self.deleted.contains(&row),
                "选中的单元格不可编辑"
            );
            bytes = bytes.saturating_sub(
                self.cells
                    .get(&(row, column))
                    .map_or(0, |value| 128 + value.as_ref().map_or(0, String::capacity)),
            );
            if result.value(row, column) != Some(batch.text.as_str()) {
                bytes = bytes.saturating_add(128 + batch.text.len());
            }
        }
        anyhow::ensure!(
            bytes <= crate::tabs::batch_edit::LIMIT,
            "待保存内容超过 8 MiB"
        );
        let batch = self.batch.take().unwrap();
        for (row, column) in batch.cells {
            if result.value(row, column) == Some(batch.text.as_str()) {
                self.cells.remove(&(row, column));
            } else {
                self.cells.insert((row, column), Some(batch.text.clone()));
            }
        }
        Ok(())
    }
}
