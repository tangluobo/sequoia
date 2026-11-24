use super::*;
use crate::tabs::ssh::storage::State;
pub(super) fn render(
    ui: &mut Ui,
    bounds: egui::Rect,
    state: &mut State,
    busy: bool,
) -> Option<Form> {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bounds));
    child.set_clip_rect(bounds.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = egui::Vec2::ZERO;
    child.painter().rect_filled(bounds, 0, Color32::WHITE);
    let focus = child.make_persistent_id("storage-lvm-focus");
    let background = child.interact(bounds, focus, egui::Sense::click());
    if !busy && background.clicked() {
        background.request_focus();
    }
    child.painter().rect_stroke(
        bounds,
        0,
        egui::Stroke::new(1., Color32::from_gray(190)),
        egui::StrokeKind::Inside,
    );
    let rows: Vec<_> = state
        .roots
        .iter()
        .flat_map(|root| {
            std::iter::once((root, 0)).chain(
                root.children
                    .iter()
                    .filter(|_| !state.collapsed.contains(&root.key))
                    .map(|n| (n, 1)),
            )
        })
        .collect();
    let mut action = None;
    let mut selected = None;
    let mut toggle = None;
    let mut scroll = None;
    if !busy
        && let Some(key) = input::take(
            &child,
            focus,
            &[
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::Home,
                egui::Key::End,
                egui::Key::Enter,
            ],
        )
    {
        let current = rows
            .iter()
            .position(|(n, _)| state.selected.as_deref() == Some(&n.key));
        if let Some(index) = input::offset(current, rows.len(), key) {
            selected = Some(rows[index].0.key.clone());
            scroll = Some(index);
        } else if let Some(index) = current {
            let (node, depth) = rows[index];
            match key {
                egui::Key::ArrowLeft => {
                    if !node.children.is_empty() && !state.collapsed.contains(&node.key) {
                        toggle = Some(node.key.clone());
                    } else if depth > 0
                        && let Some(parent) = rows[..index].iter().rposition(|(_, d)| *d == 0)
                    {
                        selected = Some(rows[parent].0.key.clone());
                        scroll = Some(parent);
                    }
                }
                egui::Key::ArrowRight => {
                    if !node.children.is_empty() {
                        if state.collapsed.contains(&node.key) {
                            toggle = Some(node.key.clone());
                        } else if let Some(child) = rows.get(index + 1) {
                            selected = Some(child.0.key.clone());
                            scroll = Some(index + 1);
                        }
                    }
                }
                egui::Key::Enter => {
                    action = match node.kind {
                        Kind::Pv => Some(Form::VolumeGroup),
                        Kind::Lv => Some(Form::ExtendLv),
                        _ => None,
                    }
                }
                _ => {}
            }
        }
    }
    egui::ScrollArea::both()
        .id_salt("storage-lvm")
        .auto_shrink([false, false])
        .show_rows(&mut child, 21., rows.len(), |ui, range| {
            if let Some(index) = scroll {
                let top = ui.cursor().top() - range.start as f32 * 21.;
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        egui::pos2(ui.cursor().left(), top + index as f32 * 21.),
                        egui::vec2(ui.available_width(), 21.),
                    ),
                    Some(egui::Align::Center),
                );
            }
            for &(node, depth) in &rows[range] {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 21.),
                    egui::Sense::click(),
                );
                if state.selected.as_deref() == Some(&node.key) || response.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        0,
                        if state.selected.as_deref() == Some(&node.key) {
                            crate::theme::TREE_SELECTED
                        } else {
                            crate::theme::TREE_HOVER
                        },
                    );
                }
                let x = rect.left() + 6. + depth as f32 * 18.;
                let arrow =
                    egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(16., 21.));
                if !node.children.is_empty() {
                    let c = arrow.center();
                    let points = if state.collapsed.contains(&node.key) {
                        vec![
                            c + egui::vec2(-2., -4.),
                            c + egui::vec2(2., 0.),
                            c + egui::vec2(-2., 4.),
                        ]
                    } else {
                        vec![
                            c + egui::vec2(-4., -2.),
                            c + egui::vec2(0., 2.),
                            c + egui::vec2(4., -2.),
                        ]
                    };
                    ui.painter().add(egui::Shape::line(
                        points,
                        egui::Stroke::new(1.4, Color32::from_gray(100)),
                    ));
                }
                ui.painter().text(
                    egui::pos2(x + 18., rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &node.label,
                    crate::theme::java_font(12.),
                    Color32::from_gray(51),
                );
                if !busy && (response.clicked() || response.secondary_clicked()) {
                    selected = Some(node.key.clone());
                    ui.memory_mut(|m| m.request_focus(focus));
                    if response.clicked()
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|p| arrow.contains(p))
                        && !node.children.is_empty()
                    {
                        toggle = Some(node.key.clone());
                    }
                }
                if !busy && response.double_clicked() {
                    action = match node.kind {
                        Kind::Pv => Some(Form::VolumeGroup),
                        Kind::Lv => Some(Form::ExtendLv),
                        _ => None,
                    };
                    if action.is_none() && !node.children.is_empty() {
                        toggle = Some(node.key.clone());
                    }
                }
                response.context_menu(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    for (label, kind, enabled) in [
                        ("创建/扩展 VG", Form::VolumeGroup, node.kind == Kind::Pv),
                        ("扩展 LV", Form::ExtendLv, node.kind == Kind::Lv),
                        (
                            "格式化/挂载",
                            Form::Mount,
                            matches!(node.kind, Kind::Pv | Kind::Lv),
                        ),
                    ] {
                        if ui
                            .add_enabled(!busy && enabled, crate::compact_menu::Item::new(label))
                            .clicked()
                        {
                            selected = Some(node.key.clone());
                            ui.memory_mut(|m| m.request_focus(focus));
                            action = Some(kind);
                            ui.close();
                        }
                    }
                });
            }
        });
    if let Some(key) = selected {
        state.selected = Some(key);
    }
    if let Some(key) = toggle
        && !state.collapsed.remove(&key)
    {
        state.collapsed.insert(key);
    }
    action
}
