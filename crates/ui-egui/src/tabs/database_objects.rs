//! Object browser state. Selection stores physical indices and is shared by all modes.
use std::{collections::BTreeSet, sync::Arc};
use tomato_core::database_objects::Catalog;
mod diagram;
pub mod window;
pub use diagram::Diagram;

pub struct State {
    pub catalog: Arc<Catalog>,
    pub fields_pending: bool,
    pub search: String,
    pub filtered: Vec<usize>,
    pub selected: BTreeSet<usize>,
    pub selection_order: Vec<usize>,
    pub anchor: Option<usize>,
    pub primary: Option<usize>,
    pub diagram: Option<Diagram>,
    query: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            catalog: Arc::new(Catalog::default()),
            fields_pending: false,
            search: String::new(),
            filtered: Vec::new(),
            selected: BTreeSet::new(),
            selection_order: Vec::new(),
            anchor: None,
            primary: None,
            diagram: None,
            query: String::new(),
        }
    }
}
impl State {
    #[cfg(test)]
    pub fn receive(&mut self, catalog: Catalog) {
        // Metadata replacement can change row order. Never retain index-based edits.
        self.selected.clear();
        self.selection_order.clear();
        self.primary = None;
        self.anchor = None;
        self.catalog = Arc::new(catalog);
        self.fields_pending = false;
        self.filter();
    }
    pub fn filter(&mut self) {
        self.diagram = None;
        let keyword = self.search.trim_matches(|c: char| c <= ' ').to_lowercase();
        self.filtered.clear();
        self.filtered
            .extend(
                self.catalog
                    .objects
                    .iter()
                    .enumerate()
                    .filter_map(|(index, object)| {
                        (keyword.is_empty()
                            || object.table.name.to_lowercase().contains(&keyword)
                            || object.comment.to_lowercase().contains(&keyword)
                            || object.fields.iter().any(|field| {
                                field.name.to_lowercase().contains(&keyword)
                                    || field.comment.to_lowercase().contains(&keyword)
                            }))
                        .then_some(index)
                    }),
            );
        // Original populateDetailView replaces TableView.items, firing its
        // selection listener even while the detail view is hidden. Searching
        // therefore clears selection, but deliberately retains the anchor.
        self.selected.clear();
        self.selection_order.clear();
        self.primary = None;
        self.query = keyword;
    }
    pub fn diagram(&mut self) -> &Diagram {
        self.diagram
            .get_or_insert_with(|| Diagram::new(&self.catalog, &self.filtered))
    }
    pub fn click(&mut self, index: usize, control: bool, shift: bool) {
        if !self.filtered.contains(&index) {
            return;
        }
        if shift && let Some(anchor) = self.anchor {
            let Some(anchor) = self.filtered.iter().position(|index| *index == anchor) else {
                return;
            };
            let Some(end) = self.filtered.iter().position(|row| *row == index) else {
                return;
            };
            self.selected.clear();
            self.selection_order.clear();
            self.selected.extend(
                self.filtered[anchor.min(end)..=anchor.max(end)]
                    .iter()
                    .copied(),
            );
            self.selection_order.extend(
                self.filtered[anchor.min(end)..=anchor.max(end)]
                    .iter()
                    .copied(),
            );
        } else if control {
            if self.selected.remove(&index) {
                self.selection_order.retain(|item| *item != index);
                if self.primary == Some(index) {
                    self.primary = self.selection_order.first().copied();
                }
                return;
            }
            self.selected.insert(index);
            self.selection_order.push(index);
            self.anchor = Some(index);
        } else {
            self.selected.clear();
            self.selection_order.clear();
            self.selected.insert(index);
            self.selection_order.push(index);
            self.anchor = Some(index);
        }
        self.primary = Some(index);
    }
    pub fn select_all(&mut self) {
        self.selected = self.filtered.iter().copied().collect();
        self.selection_order = self.filtered.clone();
        self.primary = self.selection_order.first().copied();
    }
    pub fn copy_tables(&self, connection: &str, database: &str, namespace: &str) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let mut output = format!("TOMATO_COPY_TABLES\n{connection}\n{database}\n{namespace}");
        for index in &self.selection_order {
            if let Some(object) = self.catalog.objects.get(*index) {
                output.push('\n');
                output.push_str(&object.table.name);
            }
        }
        Some(output)
    }
    pub fn status(&self) -> String {
        let tables = self
            .filtered
            .iter()
            .filter(|index| !self.catalog.objects[**index].view)
            .count();
        let views = self.filtered.len() - tables;
        let mut text = format!(
            "共 {} 个对象（表 {tables}，视图 {views}）",
            self.filtered.len()
        );
        if !self.catalog.foreign_keys.is_empty() {
            text.push_str(&format!(
                "  |  {} 个外键关系",
                self.catalog.foreign_keys.len()
            ));
        }
        if !self.selected.is_empty() {
            text.push_str(&format!("  |  已选中 {} 个", self.selected.len()));
        }
        if !self.query.is_empty() {
            text.push_str(&format!(
                "  |  已筛选（共 {} 个）",
                self.catalog.objects.len()
            ));
            if self.fields_pending {
                text.push_str("  |  字段加载中...");
            }
        }
        text
    }
}
#[cfg(test)]
mod tests;
