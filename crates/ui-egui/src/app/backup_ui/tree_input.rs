use super::{draft::Draft, selection::Group};
use tomato_core::backup::Kind;
pub(super) fn rows(group: &Group) -> usize {
    1 + if group.expanded {
        2 + if group.custom_expanded {
            group.items.len()
        } else {
            0
        }
    } else {
        0
    }
}
fn at(groups: &[Group; 4], mut index: usize) -> Option<(Kind, usize)> {
    for group in groups {
        let count = rows(group);
        if index < count {
            return Some((group.kind, index));
        }
        index -= count;
    }
    None
}
fn position(groups: &[Group; 4], key: (Kind, usize)) -> usize {
    let mut start = 0;
    for group in groups {
        if group.kind == key.0 {
            return start + key.1.min(rows(group) - 1);
        }
        start += rows(group);
    }
    0
}
pub(super) fn toggle(group: &mut Group, index: usize, selected: &mut Option<(Kind, usize)>) {
    if index == 0 {
        group.expanded = !group.expanded;
        if !group.expanded && selected.is_some_and(|(kind, row)| kind == group.kind && row > 0) {
            *selected = Some((group.kind, 0));
        }
    } else if index == 2 {
        group.custom_expanded = !group.custom_expanded;
        if !group.custom_expanded
            && selected.is_some_and(|(kind, row)| kind == group.kind && row > 2)
        {
            *selected = Some((group.kind, 2));
        }
    }
}
pub(super) fn keyboard(ui: &egui::Ui, draft: &mut Draft, id: egui::Id, height: f32) {
    if !ui.memory(|memory| memory.has_focus(id)) {
        return;
    }
    let before = draft.tree_selected;
    let key = draft.tree_selected.unwrap_or((Kind::Table, 0));
    let index = position(&draft.groups, key);
    let count = draft.groups.iter().map(rows).sum::<usize>();
    if let Some(target) = ui.input(|input| {
        if input.key_pressed(egui::Key::ArrowDown) {
            Some(index.saturating_add(1).min(count.saturating_sub(1)))
        } else if input.key_pressed(egui::Key::ArrowUp) {
            Some(index.saturating_sub(1))
        } else if input.key_pressed(egui::Key::Home) {
            Some(0)
        } else if input.key_pressed(egui::Key::End) {
            Some(count.saturating_sub(1))
        } else {
            None
        }
    }) {
        draft.tree_selected = at(&draft.groups, target);
    } else if ui.input(|input| input.key_pressed(egui::Key::ArrowRight)) {
        if let Some(group) = draft.groups.iter_mut().find(|group| group.kind == key.0) {
            if key.1 == 0 {
                if !group.expanded {
                    group.expanded = true;
                } else {
                    draft.tree_selected = Some((key.0, 1));
                }
            } else if key.1 == 2 {
                if !group.custom_expanded {
                    group.custom_expanded = true;
                } else if !group.items.is_empty() {
                    draft.tree_selected = Some((key.0, 3));
                }
            }
        }
    } else if ui.input(|input| input.key_pressed(egui::Key::ArrowLeft))
        && let Some(group) = draft.groups.iter_mut().find(|group| group.kind == key.0)
    {
        if key.1 == 0 {
            group.expanded = false;
        } else if key.1 == 2 && group.custom_expanded && !group.items.is_empty() {
            group.custom_expanded = false;
        } else {
            draft.tree_selected = Some((key.0, if key.1 > 2 { 2 } else { 0 }));
        }
    }
    if draft.tree_selected != before
        && let Some(selected) = draft.tree_selected
    {
        let top = position(&draft.groups, selected) as f32 * 26.;
        if top < draft.tree_scroll {
            draft.tree_scroll = top;
        } else if top + 26. > draft.tree_scroll + height {
            draft.tree_scroll = (top + 26. - height).max(0.);
        }
    }
}
