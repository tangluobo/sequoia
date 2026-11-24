use super::dialogs::confirm;
use super::*;
mod grid;
mod menu;
mod selection;
pub(crate) fn selected(workspace: &Workspace, section: Section) -> Vec<Vec<String>> {
    workspace
        .panes
        .get(&section)
        .and_then(|p| {
            p.table.as_ref().map(|t| {
                p.selected
                    .iter()
                    .filter_map(|i| t.rows.get(*i).cloned())
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub(crate) fn render(
    ui: &mut Ui,
    workspace: &mut Workspace,
    section: Section,
    connection: &tomato_core::Connection,
) -> Option<Intent> {
    if section == Section::Storage {
        super::storage::render(ui, workspace);
        return None;
    }
    let mut intent = None;
    let busy = workspace.busy(section);
    if section == Section::Ports {
        let pane = workspace.panes.entry(section).or_default();
        ui.spacing_mut().item_spacing.y = 0.;
        egui::Frame::new()
            .fill(Color32::from_gray(250))
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 8.;
                ui.spacing_mut().interact_size.y = 17.;
                ui.style_mut().override_font_id = Some(crate::theme::java_font(11.));
                ui.horizontal(|ui| {
                    ui.label("协议:");
                    let tcp = ui.checkbox(&mut pane.tcp, "TCP").changed();
                    let udp = ui.checkbox(&mut pane.udp, "UDP").changed();
                    if tcp || udp {
                        pane.selected.clear();
                        pane.anchor = None;
                        pane.status = "共 ".into();
                    }
                });
            });
        ui.spacing_mut().item_spacing.y = 0.;
    }
    let bounded = workspace.standalone == Some(Section::Ports);
    let (chosen, mut action) = grid::render(
        ui,
        workspace.panes.entry(section).or_default(),
        section,
        busy,
        bounded,
    );
    if let Some(row) = chosen
        && section == Section::Docker
    {
        if row
            .get(3)
            .is_some_and(|s| s.to_ascii_lowercase().starts_with("up"))
        {
            action = Some("进入终端".into());
        } else {
            action = Some("启动".into());
        }
    }
    if let Some(action) = action {
        let rows = selected(workspace, section);
        let ids: Vec<_> = rows.iter().filter_map(|r| r.first().cloned()).collect();
        match action.as_str() {
            "刷新" => workspace.panes.entry(section).or_default().refresh = true,
            "复制容器ID" => ui.ctx().copy_text(ids.join("\n")),
            "复制端口" => ui.ctx().copy_text(
                rows.iter()
                    .filter_map(|r| r.get(2).cloned())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            "进入终端" | "查看实时日志" if section == Section::Docker => {
                if let Some(id) = ids.first()
                    && let Ok(command) =
                        management::container_terminal(id, action == "查看实时日志")
                {
                    intent = Some(Intent::SshCommand {
                        connection: connection.clone(),
                        command,
                        title: format!("{} - {}", action, id),
                    });
                }
            }
            "属性" if section == Section::Docker => {
                if let Some(id) = ids.first() {
                    intent = Some(Intent::SshInspect {
                        connection: connection.clone(),
                        id: id.clone(),
                    });
                }
            }
            "查看日志" if section == Section::Docker => {
                if let Some(id) = ids.first() {
                    workspace.enqueue(
                        section,
                        Operation::Manage(if action == "属性" {
                            ManagementAction::DockerInspect(id.clone())
                        } else {
                            ManagementAction::DockerLogs(id.clone())
                        }),
                    );
                }
            }
            _ if section == Section::Docker => {
                let verb = match action.as_str() {
                    "启动" => "start",
                    "停止" => "stop",
                    "重启" => "restart",
                    "删除" => "rm",
                    _ => "rm -f",
                };
                let op = Operation::Manage(ManagementAction::Docker {
                    action: verb.into(),
                    ids,
                });
                if verb.starts_with("rm") {
                    confirm(workspace, "确认删除容器", op);
                } else {
                    workspace.enqueue(section, op);
                }
            }
            "停止" if section == Section::Ports => {
                let pids = rows.iter().filter_map(|r| r.get(3)?.parse().ok()).collect();
                confirm(
                    workspace,
                    "确认停止所选进程",
                    Operation::Manage(ManagementAction::StopProcesses(pids)),
                );
            }
            "加入防火墙白名单" => {
                for row in rows {
                    if let Some(port) = row.get(2).and_then(|v| v.parse().ok()) {
                        workspace.enqueue(
                            section,
                            Operation::Manage(ManagementAction::AllowPort {
                                port,
                                udp: row[0].starts_with("udp"),
                            }),
                        );
                    }
                }
            }
            _ if section == Section::Services => {
                let verb = match action.as_str() {
                    "启动" => "start",
                    "停止" => "stop",
                    "重启" => "restart",
                    "启用" => "enable",
                    "禁用" => "disable",
                    "状态" => "status",
                    _ => "logs",
                };
                for id in ids {
                    workspace.enqueue(
                        section,
                        Operation::Manage(ManagementAction::Service {
                            action: verb.into(),
                            name: id,
                        }),
                    );
                }
            }
            _ => {}
        }
    }
    intent
}

pub(crate) fn paint_columns(
    ui: &Ui,
    y: f32,
    mut x: f32,
    values: &[String],
    widths: &[f32],
    header: bool,
) {
    for (value, width) in values.iter().zip(widths) {
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(*width, 24.));
        ui.painter()
            .with_clip_rect(rect.intersect(ui.clip_rect()))
            .text(
                if header {
                    rect.center()
                } else {
                    rect.left_center() + egui::vec2(5., 0.)
                },
                if header {
                    egui::Align2::CENTER_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                value,
                crate::theme::java_font(if header { 12. } else { 11. }),
                Color32::from_gray(51),
            );
        x += width;
    }
}

pub(crate) fn select(
    selected: &mut std::collections::BTreeSet<usize>,
    anchor: &mut Option<usize>,
    index: usize,
    modifiers: egui::Modifiers,
    secondary: bool,
) {
    if modifiers.shift {
        let start = anchor.unwrap_or(index);
        if !modifiers.ctrl {
            selected.clear();
        }
        selected.extend(start.min(index)..=start.max(index));
    } else if modifiers.ctrl && !secondary {
        if !selected.insert(index) {
            selected.remove(&index);
        }
        *anchor = Some(index);
    } else {
        if !secondary || !selected.contains(&index) {
            selected.clear();
        }
        selected.insert(index);
        *anchor = Some(index);
    }
}
