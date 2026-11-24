//! Open remote Markdown in the shared editor using the original authenticated actor.
use super::*;
mod close;
#[cfg(test)]
mod tests;
mod view;
impl TomatoApp {
    pub(super) fn open_file_document(&mut self, owner: &str, path: String) {
        let embedded = self
            .tabs
            .iter()
            .find(|t| t.id == owner)
            .is_some_and(view::embedded);
        let source_id = format!("session-file:{owner}:{path}");
        let client = self
            .tabs
            .iter()
            .find(|tab| tab.id == owner)
            .and_then(|tab| {
                if let Panel::Terminal { session, .. } = &tab.panel {
                    session.document_client()
                } else {
                    None
                }
            });
        let Some(client) = client else {
            self.error = Some("原文件连接已关闭".into());
            return;
        };
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.source_id == source_id) {
            if let Panel::Editor { markdown, .. } = &mut tab.panel {
                markdown.file_client = Some(client);
            }
            let id = tab.id.clone();
            self.select_file_document(owner, &id, embedded);
            return;
        }
        let id = Connection::new("TOOL").id;
        self.tabs.push(Tab {
            id: id.clone(),
            source_id,
            title: path.rsplit('/').next().unwrap_or(&path).into(),
            panel: Panel::Editor {
                path: PathBuf::from(&path),
                text: String::new(),
                saved: String::new(),
                markdown: Box::new(crate::tabs::markdown::State {
                    file_client: Some(client.clone()),
                    file_owner: Some(owner.into()),
                    file_embedded: embedded,
                    ..Default::default()
                }),
            },
            busy: false,
            error: None,
        });
        self.submit(
            &id,
            Command::FileDocument {
                client: Some(client),
                action: tomato_services::files::document::Action::Read(path),
            },
            Purpose::Read,
        );
        self.select_file_document(owner, &id, embedded);
    }
    pub(super) fn rebind_file_documents(&mut self, owner: &str) {
        let client = self
            .tabs
            .iter()
            .find(|t| t.id == owner)
            .and_then(|t| match &t.panel {
                Panel::Terminal { session, .. } => session.document_client(),
                _ => None,
            });
        if let Some(client) = client {
            for tab in &mut self.tabs {
                if let Panel::Editor { markdown, .. } = &mut tab.panel
                    && markdown.file_owner.as_deref() == Some(owner)
                {
                    markdown.file_client = Some(client.clone());
                }
            }
        }
    }
    pub(super) fn file_document_change(
        &mut self,
        owner: &str,
        change: crate::tabs::ssh::FileChange,
    ) {
        match change {
            crate::tabs::ssh::FileChange::Created(path) => self.open_file_document(owner, path),
            crate::tabs::ssh::FileChange::Renamed { from, to } => {
                let from = from.trim_end_matches('/');
                let to = to.trim_end_matches('/');
                for tab in &mut self.tabs {
                    if let Panel::Editor { path, markdown, .. } = &mut tab.panel
                        && markdown.file_owner.as_deref() == Some(owner)
                    {
                        let value = path.to_string_lossy().replace('\\', "/");
                        if value == from
                            || value
                                .strip_prefix(from)
                                .is_some_and(|tail| tail.starts_with('/'))
                        {
                            let next = format!("{to}{}", &value[from.len()..]);
                            *path = PathBuf::from(&next);
                            tab.source_id = format!("session-file:{owner}:{next}");
                            tab.title = next.rsplit('/').next().unwrap_or(&next).into();
                            markdown.resource_generation =
                                markdown.resource_generation.wrapping_add(1);
                            markdown.clear_images();
                        }
                    }
                }
            }
        }
    }
}
