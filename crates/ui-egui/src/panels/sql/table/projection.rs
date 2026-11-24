//! Stable editable physical row IDs. Filtering and sorting belong to the server.
use super::*;
use std::borrow::Cow;
pub(super) fn value(state: &Workbench, row: usize, column: usize) -> Option<&Cell> {
    let page = state.page.as_ref()?;
    if row < page.rows.len() {
        state.edits.get(&row).unwrap_or(&page.rows[row]).get(column)
    } else {
        state
            .inserted
            .get(row - page.rows.len())?
            .get(column)?
            .as_ref()
    }
}
pub(super) fn text(state: &Workbench, row: usize, column: usize) -> Option<Cow<'_, str>> {
    if let Some(text) = state
        .batch
        .as_ref()
        .and_then(|batch| batch.value(row, column))
    {
        return Some(Cow::Borrowed(text));
    }
    match value(state, row, column)? {
        Cell::Null => None,
        Cell::Text(value) => Some(Cow::Borrowed(value)),
        value => Some(Cow::Owned(value.display())),
    }
}
pub(super) fn order(state: &Workbench) -> Vec<usize> {
    let total = state.page.as_ref().map_or(0, |page| page.rows.len()) + state.inserted.len();
    let mut rows: Vec<_> = state
        .table_view
        .projection
        .order
        .iter()
        .copied()
        .filter(|row| *row < total)
        .collect();
    if rows.is_empty() {
        return (0..total).collect();
    }
    if rows.len() < total {
        let present: std::collections::BTreeSet<_> = rows.iter().copied().collect();
        rows.extend((0..total).filter(|row| !present.contains(row)));
    }
    rows
}
pub(super) fn rows(state: &Workbench) -> Vec<usize> {
    order(state)
}
