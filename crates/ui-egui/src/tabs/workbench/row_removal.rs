//! Immediate row deletion must preserve unrelated unsaved cells and inserted drafts.
use super::*;
impl Workbench {
    pub fn remove_rows(&mut self, table: &TableId, rows: Vec<(usize, Vec<Cell>)>, affected: u64) {
        let Some(page) = &mut self.page else { return };
        if &page.definition.table != table {
            return;
        }
        let removed: Vec<_> = rows
            .into_iter()
            .filter_map(|(index, original)| {
                (page.rows.get(index) == Some(&original)).then_some(index)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if removed.is_empty() {
            self.needs_refresh = true;
            self.status = "删除已执行，当前页已变化，请刷新".into();
            return;
        }
        let remap = |index: usize| {
            (!removed.contains(&index)).then(|| index - removed.partition_point(|row| *row < index))
        };
        if let Some(batch) = &mut self.batch {
            batch.cells.retain_mut(|cell| {
                if let Some(row) = remap(cell.0) {
                    cell.0 = row;
                    true
                } else {
                    false
                }
            });
            if batch.cells.is_empty() {
                self.batch = None;
            }
        }
        let mut index = 0;
        page.rows.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
        page.total = page.total.saturating_sub(affected);
        self.edits = std::mem::take(&mut self.edits)
            .into_iter()
            .filter_map(|(row, values)| remap(row).map(|row| (row, values)))
            .collect();
        self.deleted = std::mem::take(&mut self.deleted)
            .into_iter()
            .filter_map(remap)
            .collect();
        self.table_view.projection.order = std::mem::take(&mut self.table_view.projection.order)
            .into_iter()
            .filter_map(remap)
            .collect();
        self.table_view.selector_rows.clear();
        self.table_view.row_mode = false;
        self.table_view.selector_focus = false;
        self.table_view.row_anchor = None;
        self.selected.clear();
        self.table_view.cells.clear();
        self.table_view.focus = None;
        self.table_view.anchor = None;
        self.table_view.row_drag = None;
        self.table_view.press = None;
        self.status = format!("已删除 {affected} 条数据");
    }
}
