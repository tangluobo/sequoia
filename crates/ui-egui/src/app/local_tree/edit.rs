use super::*;
impl TomatoApp {
    pub(in crate::app) fn local_edit_start(
        &mut self,
        connection: Connection,
        path: PathBuf,
        kind: Kind,
    ) {
        if self.local_edit.as_ref().is_some_and(|edit| edit.busy)
            || self
                .pending
                .values()
                .any(|pending| matches!(pending.purpose, Purpose::LocalMutation { .. }))
        {
            return;
        }
        let deleting = if kind == Kind::Delete {
            self.local_selection
                .iter()
                .filter(|(id, _)| *id == connection.id)
                .map(|(_, path)| path.clone())
                .collect()
        } else {
            Vec::new()
        };
        let name = match kind {
            Kind::Markdown => "新文档.md".into(),
            Kind::Directory => "新建目录".into(),
            Kind::Rename => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Kind::Delete => String::new(),
        };
        self.local_edit = Some(Edit {
            connection,
            path,
            kind,
            name,
            focus: true,
            busy: false,
            deleting,
        });
    }
    pub(in crate::app) fn local_edit_submit(&mut self) {
        use tomato_services::files::tree::Action;
        let Some(edit) = &mut self.local_edit else {
            return;
        };
        if edit.busy {
            return;
        }
        let name = edit.name.trim().to_owned();
        if edit.kind != Kind::Delete && name.is_empty() {
            return;
        }
        let action = match edit.kind {
            Kind::Directory => Action::CreateDirectory {
                parent: edit.path.clone(),
                name,
            },
            Kind::Markdown => Action::CreateMarkdown {
                parent: edit.path.clone(),
                name,
            },
            Kind::Rename => Action::Rename {
                path: edit.path.clone(),
                name,
            },
            Kind::Delete => Action::Delete {
                paths: if edit.deleting.is_empty() {
                    vec![edit.path.clone()]
                } else {
                    edit.deleting.clone()
                },
            },
        };
        let connection = edit.connection.clone();
        let command = self.tree_mutation(&connection, action);
        match self.queue.submit(command) {
            Ok(id) => {
                self.local_edit.as_mut().unwrap().busy = true;
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: format!("local-mutation:{}", connection.id),
                        purpose: Purpose::LocalMutation { connection },
                    },
                );
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(in crate::app) fn local_mutation_reply(
        &mut self,
        connection: Connection,
        result: anyhow::Result<Output>,
    ) {
        if let Some(edit) = &mut self.local_edit {
            edit.busy = false;
        }
        match result {
            Ok(Output::LocalTree(changed)) => {
                self.local_edit = None;
                let renamed = changed.renamed.is_some();
                if let Some((old, new)) = &changed.renamed {
                    self.local_tree_renamed(&connection, old, new);
                }
                let object = tomato_services::files::object_tree::is_object(&connection);
                let close_remote: Vec<_> = if object {
                    self.tabs
                        .iter()
                        .filter_map(|tab| match &tab.panel {
                            Panel::Editor { path, markdown, .. }
                                if markdown
                                    .remote
                                    .as_ref()
                                    .is_some_and(|c| c.id == connection.id)
                                    && (changed
                                        .renamed
                                        .as_ref()
                                        .is_some_and(|(old, _)| path.starts_with(old))
                                        || changed.deleted.iter().any(|p| path.starts_with(p))) =>
                            {
                                Some(tab.id.clone())
                            }
                            _ => None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                for id in close_remote {
                    self.close(&id);
                }
                if let Some((old, new)) = changed.renamed {
                    for tab in &mut self.tabs {
                        if let Panel::Editor { path, markdown, .. } = &mut tab.panel
                            && !object
                            && markdown.remote.is_none()
                            && let Ok(suffix) = path.strip_prefix(&old)
                        {
                            *path = if suffix.as_os_str().is_empty() {
                                new.clone()
                            } else {
                                new.join(suffix)
                            };
                            tab.title = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            tab.source_id = format!("file:{}", path.display());
                            markdown.resource_generation =
                                markdown.resource_generation.wrapping_add(1);
                            markdown.clear_images();
                        }
                    }
                }
                let close: Vec<_> = self
                    .tabs
                    .iter()
                    .filter_map(|tab| match &tab.panel {
                        Panel::Editor { path, markdown, .. }
                            if !object
                                && markdown.remote.is_none()
                                && changed.deleted.iter().any(|p| path.starts_with(p)) =>
                        {
                            Some(tab.id.clone())
                        }
                        _ => None,
                    })
                    .collect();
                for id in close {
                    self.close(&id);
                }
                if !renamed {
                    self.local_selection.clear();
                }
                for parent in changed.refresh {
                    if renamed {
                        // The subtree paths were moved above; refresh only the parent listing.
                        if let Some(host) = self.local_trees.get_mut(&connection.id) {
                            host.directories.remove(&parent);
                        }
                    }
                    self.local_tree_open(&connection, Some(parent), !renamed);
                }
                if let Some(path) = changed.created {
                    self.open_tree_editor(connection.clone(), path);
                }
                if !changed.errors.is_empty() {
                    self.error = Some(changed.errors.join("\n"));
                }
            }
            Err(error) => self.error = Some(format!("文件操作失败：{error:#}")),
            _ => self.error = Some("文件操作返回了不匹配的结果".into()),
        }
    }
    pub(in crate::app) fn local_tree_dialog(&mut self, ctx: &egui::Context) {
        let Some(edit) = &mut self.local_edit else {
            return;
        };
        if edit.kind == Kind::Rename {
            return;
        }
        let title = match edit.kind {
            Kind::Directory => "新建目录",
            Kind::Markdown => "新建 Markdown 文档",
            Kind::Delete => "确认删除",
            Kind::Rename => unreachable!(),
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(title)
            .id(egui::Id::new("local-tree-edit"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!edit.busy, |ui| {
                    if edit.kind == Kind::Delete {
                        ui.label("确定要删除以下项目吗？此操作不可恢复！");
                        ScrollArea::vertical().max_height(240.).show(ui, |ui| {
                            for path in if edit.deleting.is_empty() {
                                std::slice::from_ref(&edit.path)
                            } else {
                                edit.deleting.as_slice()
                            } {
                                ui.label(path.display().to_string());
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(if edit.kind == Kind::Directory {
                                "目录名："
                            } else {
                                "文件名："
                            });
                            let response = ui.text_edit_singleline(&mut edit.name);
                            if edit.focus {
                                response.request_focus();
                                edit.focus = false;
                            }
                            submit |= response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        });
                    }
                    ui.horizontal(|ui| {
                        submit |= ui
                            .button(if edit.kind == Kind::Delete {
                                "确认删除"
                            } else {
                                "确定"
                            })
                            .clicked();
                        cancel |= ui.button("取消").clicked();
                    });
                    cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
                if edit.busy {
                    ui.spinner();
                }
            });
        if cancel {
            self.local_edit = None;
        } else if submit {
            self.local_edit_submit();
        }
    }
}
