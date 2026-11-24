use super::*;
mod native;
pub(crate) enum Kind {
    Save(String),
    Directory,
    Rename(Entry),
    Delete(Vec<Entry>),
}
pub(crate) struct Dialog {
    pub kind: Kind,
    connection: String,
    namespace: String,
    scope: Option<Scope>,
    text: String,
    focus: bool,
    editor: crate::tabs::text_state::Editor,
}
impl Dialog {
    pub fn save(tab: String) -> Self {
        Self {
            kind: Kind::Save(tab),
            connection: String::new(),
            namespace: String::new(),
            scope: None,
            text: "查询1".into(),
            focus: true,
            editor: Default::default(),
        }
    }
    pub fn with_name(mut self, name: String) -> Self {
        self.text = name;
        self
    }
    pub fn directory(connection: String, namespace: String, scope: Scope) -> Self {
        Self {
            kind: Kind::Directory,
            connection,
            namespace,
            scope: Some(scope),
            text: "新目录".into(),
            focus: true,
            editor: Default::default(),
        }
    }
    pub fn rename(connection: String, namespace: String, scope: Scope, entry: Entry) -> Self {
        let text = entry.name.clone();
        Self {
            kind: Kind::Rename(entry),
            connection,
            namespace,
            scope: Some(scope),
            text,
            focus: true,
            editor: Default::default(),
        }
    }
    pub fn delete(
        connection: String,
        namespace: String,
        scope: Scope,
        entries: Vec<Entry>,
    ) -> Self {
        Self {
            kind: Kind::Delete(entries),
            connection,
            namespace,
            scope: Some(scope),
            text: String::new(),
            focus: true,
            editor: Default::default(),
        }
    }
}
impl TomatoApp {
    pub(in crate::app) fn query_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.queries.dialog.take() else {
            return;
        };
        let (title, label) = match &dialog.kind {
            Kind::Save(_) => ("保存查询", "查询名称："),
            Kind::Directory => ("新建目录", "目录名称："),
            Kind::Rename(entry) => {
                if entry.directory {
                    ("重命名目录", "新名称：")
                } else {
                    ("重命名查询", "新名称：")
                }
            }
            Kind::Delete(entries) => {
                if entries.len() == 1 && entries[0].directory {
                    ("删除目录", "")
                } else {
                    ("删除查询", "")
                }
            }
        };
        let (accepted, canceled) = native::show(ctx, &mut dialog, title, label, &mut self.icons);
        if accepted || canceled {
            dialog.editor.release(ctx);
        }
        if canceled {
            if let Kind::Save(id) = &dialog.kind
                && let Some(Tab {
                    panel: Panel::Sql { workbench, .. },
                    ..
                }) = self.tabs.iter_mut().find(|tab| &tab.id == id)
                && let Some(document) = &mut workbench.document
            {
                document.close_after_save = false;
            }
            return;
        }
        if !accepted {
            self.queries.dialog = Some(dialog);
            return;
        }
        let result = match dialog.kind {
            Kind::Save(id) => self.save_query_named(&id, &dialog.text),
            kind => {
                let scope = dialog.scope.unwrap();
                let path = scope.path.clone();
                let request = match kind {
                    Kind::Directory => Request::CreateDirectory {
                        scope,
                        name: dialog.text,
                    },
                    Kind::Rename(entry) => Request::Rename {
                        scope,
                        entry,
                        name: dialog.text,
                    },
                    Kind::Delete(entries) => Request::Delete { scope, entries },
                    Kind::Save(_) => unreachable!(),
                };
                self.query_command(
                    &dialog.connection,
                    &dialog.namespace,
                    request,
                    JobKind::Change { path },
                )
            }
        };
        if let Err(error) = result {
            self.error = Some(format!("查询操作失败：{error:#}"));
        }
    }
}
