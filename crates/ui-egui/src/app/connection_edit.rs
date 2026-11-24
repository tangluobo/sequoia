//! Persistent connection-tree edits publish their candidate only after repository success.
use super::*;
pub(super) struct Rename {
    pub id: String,
    pub name: String,
    pub focus: bool,
}
impl TomatoApp {
    pub(super) fn rename_connection(&mut self, id: &str) {
        if let Some(c) = self.store.connections.iter().find(|c| c.id == id) {
            self.connection_rename = Some(Rename {
                id: c.id.clone(),
                name: c.name.clone(),
                focus: true,
            });
        }
    }
    pub(super) fn submit_connection_name(&mut self) {
        let Some(rename) = &self.connection_rename else {
            return;
        };
        let name = rename.name.trim().to_owned();
        if name.is_empty() {
            self.error = Some("名称不能为空".into());
            return;
        }
        let mut next = self.store.clone();
        let Some(c) = next.connections.iter_mut().find(|c| c.id == rename.id) else {
            self.error = Some("连接已被删除".into());
            return;
        };
        c.name = name;
        if self.persist_store(next) {
            self.connection_rename = None;
        }
    }
    pub(super) fn copy_connection(&mut self, id: &str) {
        let Some(index) = self.store.connections.iter().position(|c| c.id == id) else {
            return;
        };
        let mut next = self.store.clone();
        let mut copy = next.connections[index].clone();
        copy.id = Connection::new("SSH").id;
        copy.name.push_str(" - 副本");
        next.connections.insert(index + 1, copy);
        self.persist_store(next);
    }
    pub(super) fn new_connection_at(&mut self, parent: Option<String>) {
        self.picker_parent = parent;
        self.picker = Some(0);
    }
    pub(super) fn new_folder_at(&mut self, parent: Option<String>) {
        let mut draft = Connection::new("LOCAL_DIRECTORY");
        draft.kind = None;
        draft.name.clear();
        draft.parent_id = parent;
        self.edit_connection(draft);
    }
}
