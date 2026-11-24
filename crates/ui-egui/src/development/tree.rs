//! Version folders refer to ranges of the existing sorted change list.
use super::*;
use egui::{Color32, Rect, Sense, Ui, vec2};
pub(super) struct Node {
    pub range: std::ops::Range<usize>,
    pub prefix: usize,
    pub name: std::ops::Range<usize>,
    pub children: Vec<Node>,
    pub selected: usize,
}
pub(super) fn build(changes: &[service::Change], selected: &BTreeSet<String>) -> Vec<Node> {
    fn level(
        changes: &[service::Change],
        start: usize,
        end: usize,
        from: usize,
        depth: usize,
    ) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut row = start;
        while row < end {
            let path = changes[row].path.trim_end_matches('/');
            let to = if depth < 128 {
                path[from..].find('/').map_or(path.len(), |n| from + n)
            } else {
                path.len()
            };
            let directory = to < path.len();
            let mut next = row + 1;
            if directory {
                while next < end && changes[next].path.starts_with(&path[..=to]) {
                    next += 1
                }
            }
            nodes.push(Node {
                range: row..next,
                prefix: to,
                name: from..to,
                children: if directory {
                    level(changes, row, next, to + 1, depth + 1)
                } else {
                    vec![]
                },
                selected: 0,
            });
            row = next;
        }
        nodes.sort_by(|a, b| {
            a.children
                .is_empty()
                .cmp(&b.children.is_empty())
                .then_with(|| {
                    changes[a.range.start].path[a.name.clone()]
                        .cmp(&changes[b.range.start].path[b.name.clone()])
                })
        });
        nodes
    }
    let mut nodes = level(changes, 0, changes.len(), 0, 0);
    count(&mut nodes, changes, selected);
    nodes
}
pub(super) fn count(nodes: &mut [Node], changes: &[service::Change], selected: &BTreeSet<String>) {
    for node in nodes {
        node.selected = changes[node.range.clone()]
            .iter()
            .filter(|c| selected.contains(&c.path))
            .count();
        count(&mut node.children, changes, selected);
    }
}
enum Action {
    Open(String),
    Rollback(Vec<String>),
    Ignore(String, bool),
}
pub(super) fn show(ui: &mut Ui, state: &mut State, icons: &mut Icons, height: f32) {
    let Some(versions) = &state.versions else {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "暂未选择目录",
            crate::theme::java_font(12.),
            Color32::from_gray(153),
        );
        return;
    };
    if versions.changes.is_empty() {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &versions.message,
            crate::theme::java_font(12.),
            Color32::from_gray(153),
        );
        return;
    }
    let mut rows = Vec::new();
    fn visible<'a>(
        nodes: &'a [Node],
        changes: &[service::Change],
        open: &BTreeSet<String>,
        depth: usize,
        rows: &mut Vec<(&'a Node, usize)>,
    ) {
        for node in nodes {
            rows.push((node, depth));
            let path = &changes[node.range.start].path[..node.prefix];
            if open.contains(path) {
                visible(&node.children, changes, open, depth + 1, rows)
            }
        }
    }
    visible(
        &state.version_nodes,
        &versions.changes,
        &state.version_open,
        0,
        &mut rows,
    );
    let mut toggle = None;
    let mut select = None;
    let mut action = None;
    let git = versions.git && !state.realtime;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.;
        egui::ScrollArea::vertical()
            .id_salt("development-version-tree")
            .max_height(height)
            .auto_shrink([false, false])
            .show_rows(ui, 35., rows.len(), |ui, range| {
                for row in range {
                    let (node, depth) = rows[row];
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 35.), Sense::click());
                    let range = node.range.clone();
                    let total = range.len();
                    let selected = node.selected;
                    let name = &versions.changes[node.range.start].path[node.name.clone()];
                    let prefix = &versions.changes[node.range.start].path[..node.prefix];
                    let directory = !node.children.is_empty();
                    let untracked_directory =
                        !directory && versions.changes[node.range.start].path.ends_with('/');
                    let x = rect.left() + depth as f32 * 12.;
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 0, Color32::from_gray(245));
                    }
                    if directory {
                        let open = state.version_open.contains(prefix);
                        let arrow = Rect::from_center_size(
                            egui::pos2(x + 9., rect.center().y),
                            vec2(18., 30.),
                        );
                        view::disclosure(ui, arrow.center(), open);
                        if ui
                            .interact(
                                arrow,
                                ui.id().with(("version-arrow", prefix)),
                                Sense::click(),
                            )
                            .clicked()
                        {
                            toggle = Some(prefix.to_owned());
                        }
                    }
                    let check = Rect::from_center_size(
                        egui::pos2(x + 26., rect.center().y),
                        vec2(16., 16.),
                    );
                    if git
                        && let Some(value) = crate::native_checkbox::checkbox(
                            ui,
                            check,
                            ui.id().with(("version-check", prefix)),
                            total > 0 && selected == total,
                            selected > 0 && selected < total,
                            icons,
                        )
                    {
                        select = Some((range.clone(), value));
                    }
                    icons.paint(
                        ui,
                        &view::icon(name, directory),
                        Rect::from_center_size(
                            egui::pos2(x + 46., rect.center().y),
                            vec2(16., 16.),
                        ),
                    );
                    let status =
                        (!directory).then_some(versions.changes[node.range.start].status.as_str());
                    let label = status.map_or_else(|| name.into(), |s| format!("{name}  [{s}]"));
                    ui.painter().text(
                        egui::pos2(x + 60., rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        crate::theme::java_font(12.),
                        match status {
                            Some("未添加") => Color32::from_rgb(199, 84, 80),
                            Some("新增" | "已添加") => Color32::from_rgb(87, 150, 92),
                            Some("变更") => Color32::from_rgb(10, 101, 191),
                            Some("重命名" | "拷贝") => Color32::from_rgb(126, 87, 194),
                            Some("删除" | "已删除") => Color32::from_rgb(122, 126, 133),
                            Some("冲突") => Color32::from_rgb(211, 47, 47),
                            _ => Color32::from_gray(51),
                        },
                    );
                    if response.double_clicked()
                        && ui
                            .input(|i| i.pointer.interact_pos())
                            .is_some_and(|p| p.x > x + 56.)
                    {
                        if directory {
                            toggle = Some(prefix.into())
                        } else if !untracked_directory {
                            action = Some(Action::Open(prefix.into()))
                        }
                    }
                    response.context_menu(|ui| {
                        view::menu_style(ui);
                        if ui
                            .add_enabled(git && total > 0, egui::Button::new("回滚变更"))
                            .clicked()
                        {
                            action = Some(Action::Rollback(
                                versions.changes[range.clone()]
                                    .iter()
                                    .map(|c| c.path.clone())
                                    .collect(),
                            ));
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                git && !prefix.is_empty(),
                                egui::Button::new("添加到 .gitignore"),
                            )
                            .clicked()
                        {
                            action = Some(Action::Ignore(
                                prefix.into(),
                                directory || untracked_directory,
                            ));
                            ui.close();
                        }
                    });
                }
            });
    });
    if let Some((range, value)) = select {
        for change in &versions.changes[range] {
            if value {
                state.selected_changes.insert(change.path.clone());
            } else {
                state.selected_changes.remove(&change.path);
            }
        }
        count(
            &mut state.version_nodes,
            &versions.changes,
            &state.selected_changes,
        );
    }
    if let Some(prefix) = toggle
        && !state.version_open.remove(&prefix)
    {
        state.version_open.insert(prefix);
    }
    if let Some(action) = action {
        match action {
            Action::Open(path) => {
                if let Some(root) = state.root() {
                    state.navigate(Navigation::File(root.join(path)));
                }
            }
            Action::Rollback(files) => state.confirm = Some(Confirm::Rollback(files)),
            Action::Ignore(path, directory) => {
                if let Some(root) = state.root() {
                    state.ordinary(super::Action::Git {
                        root,
                        action: service::GitAction::Ignore { path, directory },
                    });
                }
            }
        }
    }
}
