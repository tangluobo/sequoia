use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tomato_core::result_transfer::{Area, Format};
impl State {
    pub fn formatted(
        &self,
        result: &QueryResult,
        kind: Format<'_>,
        all: bool,
    ) -> anyhow::Result<String> {
        let mut rows = BTreeSet::new();
        let mut columns = BTreeSet::new();
        if all {
            rows.extend(0..self.rows);
            columns.extend(0..result.columns.len());
        } else {
            for row in 0..self.rows {
                for column in 0..self.columns {
                    if self.contains(Cell { row, column }) {
                        rows.insert(row);
                        columns.insert(column);
                    }
                }
            }
        }
        if matches!(kind, Format::Insert { .. } | Format::Update { .. })
            && rows.is_empty()
            && let Some(cell) = self.focus
        {
            rows.insert(cell.row);
        }
        let rows: Vec<_> = rows.into_iter().collect();
        let sql = matches!(kind, Format::Insert { .. } | Format::Update { .. });
        let columns: Vec<_> = self
            .view
            .columns
            .iter()
            .map(|column| column.identity)
            .filter(|column| sql || columns.contains(column))
            .collect();
        let empty = BTreeMap::new();
        tomato_core::result_transfer::format(
            Area {
                columns: &result.columns,
                rows: &rows,
                selected_columns: &columns,
                value: |row, column| self.display_value(result, self.row(row), column),
            },
            kind,
            self.editing
                .as_ref()
                .map_or(&empty, |editing| &editing.target.comments),
        )
    }
}
