//! One bounded draft, identified by full path rather than a sorted row index.
use super::*;

pub struct RenameDraft {
    pub from: String,
    pub name: String,
    pub focus: bool,
    pub(crate) editor: crate::tabs::text_state::Editor,
    pub(crate) due: Option<f64>,
    pub(crate) context: (String, String, String),
    pub(crate) drawn: bool,
    original: String,
    directory: bool,
}

impl RenameDraft {
    pub(crate) fn new(entry: &Entry, context: (&str, &str, &str), due: Option<f64>) -> Self {
        Self {
            from: entry.path.clone(),
            name: entry.name.clone(),
            original: entry.name.clone(),
            directory: entry.directory,
            focus: due.is_none(),
            editor: Default::default(),
            due,
            context: (context.0.into(), context.1.into(), context.2.into()),
            drawn: false,
        }
    }

    pub(crate) fn release(&self, ctx: &egui::Context) {
        self.editor.release(ctx);
    }
}

impl Pane {
    pub(crate) fn file_entry(&self, path: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .chain(self.columns.iter().flat_map(|(_, entries)| entries))
            .find(|entry| entry.path == path)
    }

    pub(crate) fn finish_file_rename(
        &mut self,
        ctx: &egui::Context,
        submit: bool,
    ) -> Option<Operation> {
        let edit = self.rename.take()?;
        edit.release(ctx);
        if edit.due.is_none() && edit.context.2 == "ICON" {
            self.selected.clear();
            self.anchor = None;
        }
        if !submit || edit.due.is_some() {
            return None;
        }
        let mut name = edit.name.trim();
        if edit.directory {
            name = name.strip_suffix('/').unwrap_or(name);
        }
        if name.is_empty() || name == edit.original {
            return None;
        }
        // Remote object and Unix names are not Windows download filenames.
        // Leave platform-specific restrictions to the selected file adapter.
        if matches!(name, "." | "..") || name.contains(['/', '\\', '\0', '\r', '\n']) {
            self.status = "重命名失败：名称无效，不能包含路径分隔符".into();
            return None;
        }
        Some(Operation::Rename {
            to: join(&parent(&edit.from), name),
            from: edit.from,
        })
    }
}
