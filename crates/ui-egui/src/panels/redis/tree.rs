use super::*;
use crate::tabs::redis::State;

#[derive(Clone, Copy)]
enum Menu {
    Add,
    Delete,
    Rename,
    Ttl,
    Copy,
}

fn collect<'a>(
    node: &'a Node,
    expanded: &std::collections::BTreeSet<String>,
    depth: usize,
    rows: &mut Vec<(&'a Node, usize)>,
) {
    for child in node.children.values() {
        rows.push((child, depth));
        if expanded.contains(&child.key) {
            collect(child, expanded, depth + 1, rows);
        }
    }
}

fn menu(ui: &mut Ui, count: usize) -> Option<Menu> {
    let mut action = None;
    for (index, label, enabled, value) in [
        (0, "添加Key".to_owned(), true, Menu::Add),
        (
            1,
            if count > 1 {
                format!("删除 ({count}个)")
            } else {
                "删除".into()
            },
            count > 0,
            Menu::Delete,
        ),
        (2, "重命名".into(), count == 1, Menu::Rename),
        (3, "设置TTL".into(), count == 1, Menu::Ttl),
        (4, "复制Key名".into(), count > 0, Menu::Copy),
    ] {
        if index == 1 || index == 4 {
            ui.separator();
        }
        let item = crate::compact_menu::Item::new(&label);
        if ui.add_enabled(enabled, item).clicked() {
            action = Some(value);
            ui.close();
        }
    }
    action
}

pub(super) fn render(
    ui: &mut Ui,
    view: &mut State,
    action: &mut Option<Action>,
    icons: &mut crate::icons::Icons,
) {
    let mut rows = Vec::new();
    collect(&view.root, &view.expanded, 0, &mut rows);
    let bounds = ui.available_rect_before_wrap();
    let id = ui.id().with("redis-tree-focus");
    let background = ui.interact(bounds, id, egui::Sense::click());
    let mut chosen = None;
    background.context_menu(|ui| {
        chosen = menu(ui, view.selected.len());
    });
    let active = ui.is_enabled() && ui.memory(|m| m.has_focus(id));
    let mut next = None;
    if active && !rows.is_empty() {
        let at = view
            .focused
            .as_ref()
            .or(view.anchor.as_ref())
            .and_then(|key| rows.iter().position(|(n, _)| &n.key == key))
            .unwrap_or(0);
        let modifiers = ui.input(|i| i.modifiers);
        let page = (bounds.height() / 30.).floor().max(1.) as usize;
        for (key, to) in [
            (egui::Key::ArrowUp, at.saturating_sub(1)),
            (egui::Key::ArrowDown, (at + 1).min(rows.len() - 1)),
            (egui::Key::Home, 0),
            (egui::Key::End, rows.len() - 1),
            (egui::Key::PageUp, at.saturating_sub(page)),
            (egui::Key::PageDown, (at + page).min(rows.len() - 1)),
        ] {
            if ui.input_mut(|i| i.consume_key(modifiers, key)) {
                next = Some((to, modifiers));
            }
        }
        let (node, depth) = rows[at];
        if ui.input_mut(|i| i.consume_key(modifiers, egui::Key::ArrowLeft))
            && !view.expanded.remove(&node.key)
            && depth > 0
        {
            next = (0..at)
                .rev()
                .find(|&i| rows[i].1 < depth)
                .map(|i| (i, modifiers));
        }
        if ui.input_mut(|i| i.consume_key(modifiers, egui::Key::ArrowRight))
            && !node.children.is_empty()
        {
            if view.expanded.contains(&node.key) {
                next = Some(((at + 1).min(rows.len() - 1), modifiers));
            } else {
                view.expanded.insert(node.key.clone());
            }
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
            view.selected = rows.iter().map(|(n, _)| n.key.clone()).collect();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            chosen = Some(Menu::Delete);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            chosen = Some(Menu::Rename);
        }
        if ui.input_mut(|i| i.consume_key(modifiers, egui::Key::Space)) {
            select(
                &rows,
                at,
                modifiers,
                &mut view.selected,
                &mut view.anchor,
                false,
            );
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            *action = Some(Action::Read {
                key: node.key.clone(),
            });
        }
        if ui.input_mut(|i| {
            let copied = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            if copied {
                i.events.retain(|e| !matches!(e, egui::Event::Copy));
            }
            copied || i.consume_key(egui::Modifiers::CTRL, egui::Key::C)
        }) {
            chosen = Some(Menu::Copy);
        }
    }
    if let Some((index, modifiers)) = next {
        if modifiers.shift || !(modifiers.ctrl || modifiers.command) {
            select(
                &rows,
                index,
                modifiers,
                &mut view.selected,
                &mut view.anchor,
                false,
            );
        }
        view.focused = Some(rows[index].0.key.clone());
        view.scroll_to_focus = true;
    }
    ui.spacing_mut().item_spacing.y = 0.;
    let scroll = ScrollArea::vertical()
        .id_salt("redis-key-list")
        .auto_shrink([false, false]);
    let scroll = if view.scroll_to_focus {
        if let Some(index) = view
            .focused
            .as_ref()
            .and_then(|k| rows.iter().position(|(n, _)| &n.key == k))
        {
            view.scroll_to_focus = false;
            let previous =
                egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id("redis-key-list"))
                    .unwrap_or_default()
                    .offset
                    .y;
            let top = index as f32 * 30.;
            let offset = if top < previous {
                top
            } else if top + 30. > previous + bounds.height() {
                top + 30. - bounds.height()
            } else {
                previous
            };
            scroll.vertical_scroll_offset(offset)
        } else {
            scroll
        }
    } else {
        scroll
    };
    scroll.show_rows(ui, 30., rows.len(), |ui, range| {
        for index in range {
            let (node, depth) = rows[index];
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.), egui::Sense::click());
            if view.selected.contains(&node.key) || response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    0.,
                    if view.selected.contains(&node.key) {
                        crate::theme::TREE_SELECTED
                    } else {
                        crate::theme::TREE_HOVER
                    },
                );
            }
            let x = rect.left() + depth as f32 * 20.;
            let arrow = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(16., 30.));
            if !node.children.is_empty() {
                let center = arrow.center();
                let points = if view.expanded.contains(&node.key) {
                    vec![
                        center + egui::vec2(-4., -2.),
                        center + egui::vec2(4., -2.),
                        center + egui::vec2(0., 3.),
                    ]
                } else {
                    vec![
                        center + egui::vec2(-2., -4.),
                        center + egui::vec2(3., 0.),
                        center + egui::vec2(-2., 4.),
                    ]
                };
                ui.painter().add(egui::Shape::convex_polygon(
                    points,
                    Color32::from_gray(51),
                    egui::Stroke::NONE,
                ));
            }
            let icon = egui::Rect::from_center_size(
                egui::pos2(x + 28., rect.center().y),
                egui::vec2(16., 16.),
            );
            icons.paint(
                ui,
                if node.value {
                    "images/connect/database.png"
                } else {
                    "images/connect/folder.png"
                },
                icon,
            );
            ui.painter().text(
                egui::pos2(x + 40., rect.center().y),
                egui::Align2::LEFT_CENTER,
                &node.label,
                egui::FontId::proportional(12.),
                Color32::from_gray(51),
            );
            if response.clicked() || response.secondary_clicked() {
                ui.memory_mut(|m| m.request_focus(id));
                view.focused = Some(node.key.clone());
                let on_arrow = response
                    .interact_pointer_pos()
                    .is_some_and(|p| arrow.contains(p));
                if response.clicked() && on_arrow && !node.children.is_empty() {
                    if !view.expanded.insert(node.key.clone()) {
                        view.expanded.remove(&node.key);
                    }
                } else {
                    select(
                        &rows,
                        index,
                        ui.input(|i| i.modifiers),
                        &mut view.selected,
                        &mut view.anchor,
                        response.secondary_clicked(),
                    );
                    if response.clicked() {
                        *action = Some(Action::Read {
                            key: node.key.clone(),
                        });
                    }
                }
            }
            if response.double_clicked()
                && !node.children.is_empty()
                && !response
                    .interact_pointer_pos()
                    .is_some_and(|p| arrow.contains(p))
                && !view.expanded.insert(node.key.clone())
            {
                view.expanded.remove(&node.key);
            }
            response.context_menu(|ui| {
                chosen = menu(ui, view.selected.len());
            });
        }
    });
    match chosen {
        Some(Menu::Add) => view.open_add(),
        Some(Menu::Delete) => view.open_delete(),
        Some(Menu::Rename) => view.open_rename(),
        Some(Menu::Ttl) => view.open_ttl(),
        Some(Menu::Copy) if !view.selected.is_empty() => ui
            .ctx()
            .copy_text(view.selected.iter().cloned().collect::<Vec<_>>().join("\n")),
        _ => {}
    }
}

fn select(
    rows: &[(&Node, usize)],
    index: usize,
    modifiers: egui::Modifiers,
    selected: &mut std::collections::BTreeSet<String>,
    anchor: &mut Option<String>,
    secondary: bool,
) {
    let node = rows[index].0;
    if secondary {
        if !selected.contains(&node.key) {
            selected.clear();
            selected.insert(node.key.clone());
            *anchor = Some(node.key.clone());
        }
    } else if modifiers.shift {
        let start = anchor
            .as_ref()
            .and_then(|a| rows.iter().position(|(n, _)| &n.key == a))
            .unwrap_or(index);
        if !modifiers.ctrl && !modifiers.command {
            selected.clear();
        }
        for (node, _) in &rows[start.min(index)..=start.max(index)] {
            selected.insert(node.key.clone());
        }
    } else if modifiers.ctrl || modifiers.command {
        if !selected.insert(node.key.clone()) {
            selected.remove(&node.key);
        }
        *anchor = Some(node.key.clone());
    } else {
        selected.clear();
        selected.insert(node.key.clone());
        *anchor = Some(node.key.clone());
    }
}
