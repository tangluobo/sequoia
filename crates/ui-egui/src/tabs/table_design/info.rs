//! Lazily copied foreign-key/trigger edits; unchanged pages retain one server snapshot.
use tomato_core::table_design::{alter::InfoPage, auxiliary};
#[derive(Default)]
pub struct State {
    pub rows: Option<Vec<Vec<String>>>,
    pub added: std::collections::BTreeSet<usize>,
    pub form: Option<usize>,
    pub focus_form: bool,
    pub widths: Vec<f32>,
    pub order: Vec<usize>,
    pub selected: Option<usize>,
    pub sort: Option<(usize, bool)>,
}
impl State {
    pub fn add(&mut self, before: &[Vec<String>], page: InfoPage, schema: &str) -> bool {
        if self.rows.as_ref().map_or(before.len(), Vec::len) >= 1024 {
            return false;
        }
        let rows = self.rows.get_or_insert_with(|| before.to_vec());
        let index = rows.len();
        rows.push(auxiliary::default_row(page, schema));
        self.added.insert(index);
        self.form = Some(index);
        self.focus_form = true;
        self.selected = Some(index);
        self.order = (0..rows.len()).collect();
        self.sort = None;
        true
    }
    pub fn delete(&mut self, before: &[Vec<String>]) -> bool {
        let Some(row) = self.selected else {
            return false;
        };
        let rows = self.rows.get_or_insert_with(|| before.to_vec());
        if row >= rows.len() {
            return false;
        }
        rows.remove(row);
        self.added = self
            .added
            .iter()
            .filter(|i| **i != row)
            .map(|i| if *i > row { i - 1 } else { *i })
            .collect();
        self.form = None;
        self.order = (0..rows.len()).collect();
        self.sort = None;
        self.selected = (!rows.is_empty()).then(|| row.min(rows.len() - 1));
        true
    }
    pub fn changed(&self, before: &[Vec<String>]) -> bool {
        self.rows.as_ref().is_some_and(|rows| rows != before)
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn sort(&mut self, rows: &[Vec<String>], column: usize) {
        self.sort = match self.sort {
            Some((old, false)) if old == column => Some((column, true)),
            Some((old, true)) if old == column => None,
            _ => Some((column, false)),
        };
        self.order.clear();
        self.order.extend(0..rows.len());
        if let Some((column, descending)) = self.sort {
            self.order.sort_by(|a, b| {
                let order = rows[*a][column]
                    .encode_utf16()
                    .cmp(rows[*b][column].encode_utf16());
                if descending { order.reverse() } else { order }
            });
        }
    }
}
