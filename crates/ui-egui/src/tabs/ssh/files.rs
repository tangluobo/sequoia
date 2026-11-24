use super::*;
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
    pub fn sort_files(&mut self, column: usize) {
        self.file_sort = match self.file_sort {
            Some((old, false)) if old == column => Some((column, true)),
            Some((old, true)) if old == column => None,
            _ => Some((column, false)),
        };
        self.apply_file_sort();
    }
    pub(super) fn apply_file_sort(&mut self) {
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
        self.entries.sort_by(|a, b| {
            if let Some((column, descending)) = self.file_sort {
                let order = match column {
                    1 => a.size.cmp(&b.size),
                    2 => a.modified.cmp(&b.modified),
                    3 => b.directory.cmp(&a.directory),
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                };
                if descending { order.reverse() } else { order }
            } else {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }
        });
        self.selected = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| selected.contains(&e.path).then_some(i))
            .collect();
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
}
