//! Compose Java's path bar → inner browse/editor tabs → connection status bar.
use super::*;
pub(in crate::app) struct Rendered {
    pub intent: Option<(String, Intent)>,
}
pub(super) fn embedded(tab: &Tab) -> bool {
    matches!(&tab.panel,Panel::Terminal{connection,view,..}if (matches!(connection.kind(),"SFTP"|"S3"|"ALIYUN_OSS")||connection.kind()=="LOCAL_DIRECTORY"&&connection.text("directoryType")=="S3")&&view.ssh.as_ref().is_some_and(|w|w.standalone==Some(tomato_services::terminal::ssh::tools::Section::Files)))
}
impl TomatoApp {
    pub(in crate::app) fn render_file_documents(
        &mut self,
        ui: &mut Ui,
        mut index: usize,
    ) -> Option<Rendered> {
        if !embedded(&self.tabs[index]) {
            return None;
        }
        let owner = self.tabs[index].id.clone();
        let parent_intent = if let Panel::Terminal {
            connection, view, ..
        } = &mut self.tabs[index].panel
        {
            let workspace = view.ssh.as_mut().unwrap();
            workspace.external_file_toolbar = true;
            crate::panels::terminal::tools::file_header(ui, workspace, connection, &mut self.icons)
        } else {
            None
        };
        let children:Vec<_>=self.tabs.iter().enumerate().filter_map(|(i,t)|matches!(&t.panel,Panel::Editor{markdown,..}if markdown.file_embedded&&markdown.file_owner.as_deref()==Some(&owner)).then_some(i)).collect();
        let active = match &self.tabs[index].panel {
            Panel::Terminal { view, .. } => view.ssh.as_ref().unwrap().document_active.as_deref(),
            _ => None,
        };
        let mut selected = children
            .iter()
            .position(|i| Some(self.tabs[*i].id.as_str()) == active)
            .map(|i| i + 1)
            .unwrap_or(0);
        let items: Vec<_> = std::iter::once(("浏览".into(), false))
            .chain(children.iter().map(|i| {
                let t = &self.tabs[*i];
                (
                    if t.dirty() {
                        format!("{} *", t.title)
                    } else {
                        t.title.clone()
                    },
                    true,
                )
            }))
            .collect();
        let closed =
            crate::native_tabs::closable(ui, ("file-editor-tabs", &owner), &mut selected, &items);
        let mut document = selected
            .checked_sub(1)
            .and_then(|i| children.get(i))
            .map(|i| self.tabs[*i].id.clone());
        if let Panel::Terminal { view, .. } = &mut self.tabs[index].panel {
            view.ssh.as_mut().unwrap().document_active = document.clone();
        }
        if let Some(closed) = closed
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| children.get(i))
            .map(|i| self.tabs[*i].id.clone())
        {
            if self.tabs.iter().any(|t| t.id == closed && t.dirty()) {
                self.confirmation = Some(Confirmation::Close(closed));
            } else {
                self.close(&closed);
            }
            let Some(current) = self.tabs.iter().position(|t| t.id == owner) else {
                return Some(Rendered { intent: None });
            };
            index = current;
            document = match &self.tabs[index].panel {
                Panel::Terminal { view, .. } => view.ssh.as_ref().unwrap().document_active.clone(),
                _ => None,
            };
        }
        if let Some(document) = document
            && let Some(child) = self.tabs.iter().position(|t| t.id == document)
        {
            let all = ui.available_rect_before_wrap();
            let content = egui::Rect::from_min_max(
                all.min,
                egui::pos2(all.right(), (all.bottom() - 27.).max(all.top())),
            );
            let mut editor =
                ui.new_child(egui::UiBuilder::new().id_salt(&document).max_rect(content));
            editor.set_clip_rect(content.intersect(ui.clip_rect()));
            let intent = panels::render_with_connections(
                &mut editor,
                &mut self.tabs[child],
                &mut self.icons,
                &self.store.connections,
            );
            if let Panel::Terminal {
                connection, view, ..
            } = &mut self.tabs[index].panel
            {
                let workspace = view.ssh.as_mut().unwrap();
                if let Some(pane) = workspace
                    .panes
                    .get_mut(&tomato_services::terminal::ssh::tools::Section::Files)
                {
                    pane.thumbnails.suspend();
                }
                crate::panels::terminal::tools::file_footer(
                    ui,
                    egui::Rect::from_min_max(egui::pos2(all.left(), content.bottom()), all.max),
                    workspace,
                    connection,
                );
                crate::panels::terminal::tools::file_dialogs(ui, workspace);
            }
            Some(Rendered {
                intent: parent_intent
                    .map(|i| (owner, i))
                    .or_else(|| intent.map(|i| (document, i))),
            })
        } else {
            let intent = panels::render_with_connections(
                ui,
                &mut self.tabs[index],
                &mut self.icons,
                &self.store.connections,
            );
            Some(Rendered {
                intent: parent_intent.or(intent).map(|i| (owner, i)),
            })
        }
    }
    pub(in crate::app) fn select_file_document(&mut self, owner: &str, id: &str, embedded: bool) {
        if embedded {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == owner)
                && let Panel::Terminal { view, .. } = &mut tab.panel
                && let Some(workspace) = &mut view.ssh
            {
                workspace.document_active = Some(id.into());
            }
            self.active = Some(owner.into());
        } else {
            self.active = Some(id.into());
        }
    }
}
