use super::*;
use std::collections::BTreeSet;
use std::sync::Arc;
use tomato_core::config::ImportPlan;
mod jobs;
#[cfg(test)]
mod tests;
mod view;
pub(super) use jobs::Reader;

pub(super) struct Dialog {
    plan: Arc<ImportPlan>,
    selected: BTreeSet<usize>,
    collapsed: BTreeSet<usize>,
    positioned: bool,
    focused: Option<usize>,
    focus_tree: bool,
    source: Option<PathBuf>,
    phase: Phase,
    notice_focus: bool,
}
enum Phase {
    Ready,
    Writing {
        id: u64,
        control: tomato_services::backup::Control,
        canceling: bool,
    },
    Notice {
        message: String,
        kind: crate::native_dialog::message::Kind,
    },
    Done(String),
}
impl Dialog {
    pub(super) fn new(plan: ImportPlan) -> Self {
        Self::from_shared(Arc::new(plan), None)
    }
    fn from_shared(plan: Arc<ImportPlan>, source: Option<PathBuf>) -> Self {
        let selected = (0..plan.connections.len())
            .filter(|i| !plan.duplicate(*i))
            .collect();
        Self {
            plan,
            selected,
            collapsed: BTreeSet::new(),
            positioned: false,
            focused: None,
            focus_tree: true,
            source,
            phase: Phase::Ready,
            notice_focus: true,
        }
    }
    fn notice(&mut self, message: impl Into<String>, kind: crate::native_dialog::message::Kind) {
        self.phase = Phase::Notice {
            message: message.into(),
            kind,
        };
        self.notice_focus = true;
    }
    fn writing(&self) -> bool {
        matches!(self.phase, Phase::Writing { .. })
    }
}
impl Drop for Dialog {
    fn drop(&mut self) {
        if let Phase::Writing { control, .. } = &self.phase {
            control.cancel();
        }
    }
}
impl TomatoApp {
    pub(super) fn import_dialog(&mut self, ctx: &egui::Context) {
        self.import_read_dialog(ctx);
        let Some(dialog) = &mut self.import_dialog else {
            return;
        };
        match view::render(ctx, dialog, &mut self.icons) {
            view::Action::Cancel => {
                if let Phase::Writing {
                    control, canceling, ..
                } = &mut dialog.phase
                {
                    control.cancel();
                    *canceling = true;
                } else {
                    self.import_dialog = None;
                }
            }
            view::Action::Apply => self.apply_connection_import(),
            view::Action::None => {}
        }
    }
    pub(super) fn apply_connection_import(&mut self) {
        let Some(dialog) = &self.import_dialog else {
            return;
        };
        if !matches!(dialog.phase, Phase::Ready) {
            return;
        }
        if dialog.selected.is_empty() {
            self.import_dialog.as_mut().unwrap().notice(
                "请至少选择一个要导入的项目",
                crate::native_dialog::message::Kind::Warning,
            );
            return;
        }
        if dialog.source.is_some() {
            self.commit_connection_import();
            return;
        }
        let mut next = self.store.clone();
        match dialog.plan.apply(&mut next, &dialog.selected) {
            Ok(count) => {
                if self.persist_store(next) {
                    self.import_dialog = None;
                    self.status = format!("已成功导入 {count} 个项目");
                }
            }
            Err(error) => self.error = Some(format!("导入失败: {error:#}")),
        }
    }
    pub(super) fn request_connection_delete(&mut self, id: &str) {
        let selected = if self.connection_selection.contains(id) {
            self.connection_selection.iter().cloned().collect()
        } else {
            vec![id.to_owned()]
        };
        self.confirmation = Some(Confirmation::Delete(selected));
    }
    pub(super) fn delete_connections(&mut self, ids: &[String], keep_children: bool) -> bool {
        let mut next = self.store.clone();
        if keep_children {
            let Some(folder) = next
                .connections
                .iter()
                .find(|c| ids.len() == 1 && c.id == ids[0] && c.kind.is_none())
                .cloned()
            else {
                return false;
            };
            for c in &mut next.connections {
                if c.parent_id.as_deref() == Some(&folder.id) {
                    c.parent_id = folder.parent_id.clone();
                }
            }
            next.connections.retain(|c| c.id != folder.id);
        } else {
            let tree = tomato_core::tree::ConnectionTree::new(&next.connections);
            let removed: std::collections::HashSet<_> =
                ids.iter().flat_map(|id| tree.subtree_ids(id)).collect();
            next.connections.retain(|c| !removed.contains(&c.id));
        }
        if !self.persist_store(next) {
            return false;
        }
        self.connection_selection
            .retain(|id| self.store.connections.iter().any(|c| &c.id == id));
        true
    }
}
