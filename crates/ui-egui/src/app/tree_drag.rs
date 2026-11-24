//! Connection ordering and file moves use distinct, typed drag payloads.
use super::{db_tree::Node, *};
use tomato_core::tree::DropTarget;

#[derive(Clone)]
enum Payload {
    Connection(String),
    File { connection: String, path: PathBuf },
}
pub(super) enum Action {
    Connection {
        id: String,
        target: DropTarget,
    },
    File {
        connection: Connection,
        path: PathBuf,
        parent: PathBuf,
    },
}
pub(super) fn row(
    ui: &Ui,
    response: &egui::Response,
    node: &Node<'_>,
    connections: &[Connection],
    enabled: bool,
) -> Option<Action> {
    if !enabled {
        return None;
    }
    if response.drag_started() {
        match node {
            Node::Connection(connection) => {
                response.dnd_set_drag_payload(Payload::Connection(connection.id.clone()))
            }
            Node::LocalFile(connection, entry) => response.dnd_set_drag_payload(Payload::File {
                connection: connection.id.clone(),
                path: entry.path.clone(),
            }),
            _ => {}
        }
    }
    let payload = response.dnd_hover_payload::<Payload>()?;
    let pointer = ui.input(|input| input.pointer.interact_pos())?;
    let action = match payload.as_ref() {
        Payload::Connection(id) => {
            let Node::Connection(connection) = node else {
                return None;
            };
            if &connection.id == id {
                return None;
            }
            let y = (pointer.y - response.rect.top()) / response.rect.height();
            let target = if connection.kind.is_none() && (0.25..=0.75).contains(&y) {
                DropTarget::Into(connection.id.clone())
            } else if y < 0.5 {
                DropTarget::Before(connection.id.clone())
            } else {
                DropTarget::After(connection.id.clone())
            };
            tomato_core::tree::validate_drop(connections, id, &target).ok()?;
            if matches!(target, DropTarget::Into(_)) {
                ui.painter()
                    .rect_filled(response.rect, 0, Color32::from_gray(224));
            } else {
                let y = if matches!(target, DropTarget::Before(_)) {
                    response.rect.top()
                } else {
                    response.rect.bottom()
                };
                ui.painter().line_segment(
                    [
                        egui::pos2(response.rect.left(), y),
                        egui::pos2(response.rect.right(), y),
                    ],
                    egui::Stroke::new(2., crate::theme::GREEN),
                );
            }
            Action::Connection {
                id: id.clone(),
                target,
            }
        }
        Payload::File {
            connection: id,
            path,
        } => {
            let (connection, parent) = match node {
                Node::Connection(connection) if connection.kind() == "LOCAL_DIRECTORY" => {
                    (*connection, super::local_tree::root(connection))
                }
                Node::LocalFile(connection, entry) if entry.directory => {
                    (*connection, entry.path.clone())
                }
                _ => return None,
            };
            if &connection.id != id
                || parent.starts_with(path)
                || path.parent() == Some(parent.as_path())
            {
                return None;
            }
            ui.painter()
                .rect_filled(response.rect, 0, Color32::from_gray(224));
            Action::File {
                connection: connection.clone(),
                path: path.clone(),
                parent,
            }
        }
    };
    response.dnd_release_payload::<Payload>().map(|_| action)
}
pub(super) fn root(
    ui: &Ui,
    viewport: egui::Rect,
    content_height: f32,
    offset: f32,
    enabled: bool,
) -> Option<Action> {
    if !enabled {
        return None;
    }
    let blank = egui::Rect::from_min_max(
        egui::pos2(
            viewport.left(),
            (viewport.top() + content_height - offset).max(viewport.top()),
        ),
        viewport.max,
    );
    let pointer = ui.input(|input| input.pointer.interact_pos())?;
    if !blank.contains(pointer) || blank.height() <= 0. {
        return None;
    }
    let payload = egui::DragAndDrop::payload::<Payload>(ui.ctx())?;
    let Payload::Connection(id) = payload.as_ref() else {
        return None;
    };
    ui.painter().rect_filled(blank, 0, Color32::from_gray(224));
    ui.input(|input| input.pointer.any_released())
        .then(|| Action::Connection {
            id: id.clone(),
            target: DropTarget::Root,
        })
}
impl TomatoApp {
    pub(super) fn apply_tree_drop(&mut self, action: Action) {
        let result = match action {
            Action::Connection { id, target } => {
                if self.load_failed {
                    self.error = Some("配置加载失败，保存已禁用".into());
                    return;
                }
                (|| -> anyhow::Result<()> {
                    let parent =
                        tomato_core::tree::validate_drop(&self.store.connections, &id, &target)?;
                    let mut next = self.store.clone();
                    next.connections =
                        tomato_core::tree::move_connection(&self.store.connections, &id, &target)?;
                    // Publish to the UI only after encrypted, conflict-checked persistence succeeds.
                    self.repository.save(&next)?;
                    self.store = next;
                    if let Some(parent) = parent {
                        self.collapsed.remove(&parent);
                    }
                    self.status = "连接顺序已保存".into();
                    Ok(())
                })()
            }
            Action::File {
                connection,
                path,
                parent,
            } => self
                .queue
                .submit(self.tree_mutation(
                    &connection,
                    tomato_services::files::tree::Action::Move { path, parent },
                ))
                .map(|id| {
                    self.pending.insert(
                        id,
                        Pending {
                            tab_id: format!("local-mutation:{}", connection.id),
                            purpose: Purpose::LocalMutation { connection },
                        },
                    );
                }),
        };
        if let Err(error) = result {
            self.error = Some(format!("拖放失败：{error:#}"));
        }
    }
}
