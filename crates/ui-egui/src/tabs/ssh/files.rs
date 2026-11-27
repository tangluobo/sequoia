use super::*;
use std::{cmp::Reverse, sync::Arc};

pub(crate) struct FileIndices {
    filter: String,
    entry_count: usize,
    indices: Arc<[usize]>,
}

pub(crate) fn parent(path: &str) -> String {
    let path = path.trim_end_matches('/');
    path.rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/".into() } else { p.into() })
        .unwrap_or_else(|| "/".into())
}

pub(crate) fn join(path: &str, name: &str) -> String {
    format!("{}/{name}", path.trim_end_matches('/'))
}
impl Workspace {
    pub fn standalone_files(mode: &str, path: &str) -> Self {
        let mut view = Self {
            standalone: Some(Section::Files),
            file_view: mode.into(),
            ..Default::default()
        };
        view.open(Section::Files);
        view.panes.insert(
            Section::Files,
            Pane {
                path: path.into(),
                ..Default::default()
            },
        );
        view
    }
}
impl Pane {
    pub(crate) fn visible_file_indices(&mut self) -> Arc<[usize]> {
        let cached = self.file_indices.get_or_insert_with(|| FileIndices {
            filter: String::new(),
            entry_count: 0,
            indices: Arc::default(),
        });
        if cached.filter != self.filter || cached.entry_count != self.entries.len() {
            let filter = self.filter.to_lowercase();
            cached.indices = if filter.is_empty() {
                (0..self.entries.len()).collect()
            } else {
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        entry.name.to_lowercase().contains(&filter).then_some(index)
                    })
                    .collect()
            };
            cached.filter.clone_from(&self.filter);
            cached.entry_count = self.entries.len();
        }
        cached.indices.clone()
    }

    pub fn sort_files(&mut self, column: usize) {
        self.file_sort = match self.file_sort {
            Some((old, false)) if old == column => Some((column, true)),
            Some((old, true)) if old == column => None,
            _ => Some((column, false)),
        };
        self.apply_file_sort();
    }
    pub(super) fn apply_file_sort(&mut self) {
        self.file_indices = None;
        self.file_drag = None;
        self.thumbnails.invalidate_gallery();
        let selected: std::collections::HashSet<_> = self
            .selected
            .iter()
            .filter_map(|index| self.entries.get(*index).map(|e| e.path.clone()))
            .collect();
        let anchor = self
            .anchor
            .and_then(|i| self.entries.get(i))
            .map(|e| e.path.clone());
        let cursor = self
            .file_cursor
            .and_then(|i| self.entries.get(i))
            .map(|e| e.path.clone());
        match self.file_sort {
            None => self
                .entries
                .sort_by_cached_key(|entry| (!entry.directory, entry.name.to_lowercase())),
            Some((0, false)) => self
                .entries
                .sort_by_cached_key(|entry| entry.name.to_lowercase()),
            Some((0, true)) => self
                .entries
                .sort_by_cached_key(|entry| Reverse(entry.name.to_lowercase())),
            Some((column, descending)) => self.entries.sort_by(|a, b| {
                let order = match column {
                    1 => a.size.cmp(&b.size),
                    2 => a.modified.cmp(&b.modified),
                    3 => b.directory.cmp(&a.directory),
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                };
                if descending { order.reverse() } else { order }
            }),
        }
        if !selected.is_empty() {
            self.selected = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| selected.contains(&e.path).then_some(i))
                .collect();
        } else {
            self.selected.clear();
        }
        self.anchor = anchor.and_then(|p| self.entries.iter().position(|e| e.path == p));
        self.file_cursor = cursor.and_then(|p| self.entries.iter().position(|e| e.path == p));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_sort_keeps_selection_on_the_same_path_and_cycles_to_default() {
        let mut pane = Pane {
            entries: [("z.txt", 10), ("a.txt", 90)]
                .into_iter()
                .map(|(name, size)| Entry {
                    name: name.into(),
                    path: format!("/{name}"),
                    directory: false,
                    symlink: false,
                    size,
                    modified: 0,
                    permissions: 0,
                })
                .collect(),
            ..Default::default()
        };
        pane.selected.insert(0);
        pane.anchor = Some(0);
        pane.sort_files(0);
        assert_eq!(pane.entries[0].name, "a.txt");
        assert_eq!(pane.selected.iter().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(pane.anchor, Some(1));
        pane.sort_files(0);
        assert_eq!(pane.entries[0].name, "z.txt");
        pane.sort_files(0);
        assert_eq!(pane.file_sort, None);
        assert_eq!(pane.entries[0].name, "a.txt");
        pane.sort_files(1);
        assert_eq!(pane.entries[0].size, 10);
        assert_eq!(pane.selected.iter().copied().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn file_filter_reuses_indices_until_the_filter_or_listing_changes() {
        let mut pane = Pane {
            entries: (0..20_000)
                .map(|index| Entry {
                    name: format!("İTEM-{index}"),
                    path: format!("/{index}"),
                    directory: false,
                    symlink: false,
                    size: 0,
                    modified: 0,
                    permissions: 0,
                })
                .collect(),
            ..Default::default()
        };
        let all = pane.visible_file_indices();
        assert_eq!(all.len(), 20_000);
        assert!(Arc::ptr_eq(&all, &pane.visible_file_indices()));

        pane.filter = "i\u{307}tem-19999".into();
        assert_eq!(&*pane.visible_file_indices(), &[19_999]);
        pane.entries[19_999].name = "replacement".into();
        pane.apply_file_sort();
        assert!(pane.visible_file_indices().is_empty());
    }
}
