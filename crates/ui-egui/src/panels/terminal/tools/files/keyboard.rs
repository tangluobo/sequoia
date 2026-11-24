use crate::tabs::ssh::Pane;
use egui::{Key, Modifiers, Ui};
pub(super) fn update(
    ui: &Ui,
    pane: &mut Pane,
    indices: &[usize],
    columns: usize,
    page: usize,
    horizontal: bool,
) -> Option<usize> {
    let current = pane
        .file_cursor
        .or(pane.anchor)
        .and_then(|index| indices.iter().position(|i| *i == index));
    let mut next = None;
    if let Some(pos) = target(ui, current, indices.len(), columns, page, horizontal) {
        let modifiers = ui.input(|i| i.modifiers);
        let index = indices[pos];
        if modifiers.shift {
            let anchor = pane
                .anchor
                .and_then(|a| indices.iter().position(|i| *i == a))
                .or(current)
                .unwrap_or(pos);
            if !modifiers.command {
                pane.selected.clear();
            }
            pane.selected
                .extend(indices[anchor.min(pos)..=anchor.max(pos)].iter().copied());
        } else if !modifiers.command {
            pane.selected.clear();
            pane.selected.insert(index);
            pane.anchor = Some(index);
        }
        pane.column_selected = None;
        pane.file_cursor = Some(index);
        next = Some(pos);
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space))
        && let Some(index) = pane.file_cursor
        && !pane.selected.remove(&index)
    {
        pane.selected.insert(index);
    }
    next
}

pub(super) fn target(
    ui: &Ui,
    current: Option<usize>,
    count: usize,
    columns: usize,
    page: usize,
    horizontal: bool,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    for (key, delta) in [
        (Key::ArrowUp, -(columns as isize)),
        (Key::ArrowDown, columns as isize),
        (Key::ArrowLeft, -1),
        (Key::ArrowRight, 1),
        (Key::PageUp, -(page as isize)),
        (Key::PageDown, page as isize),
        (Key::Home, isize::MIN),
        (Key::End, isize::MAX),
    ] {
        if !horizontal && matches!(key, Key::ArrowLeft | Key::ArrowRight) {
            continue;
        }
        let modifiers = ui.input(|i| i.modifiers);
        if ui.input_mut(|i| i.consume_key(modifiers, key)) {
            return Some(match delta {
                isize::MIN => 0,
                isize::MAX => count - 1,
                _ => current
                    .map(|pos| pos.saturating_add_signed(delta).min(count - 1))
                    .unwrap_or(0),
            });
        }
    }
    None
}
