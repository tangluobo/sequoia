//! Shared Java TreeView navigation. Checkboxes are separate controls: Space
//! selects a row; icons/check marks change its checked state; labels double-fold.
use super::*;
pub(super) mod widgets;
pub(super) struct Navigation {
    pub row: usize,
    pub collapsed: Option<bool>,
}
pub(super) fn visible(
    count: usize,
    topology: impl Fn(usize) -> (usize, usize),
    closed: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let mut hidden = None;
    (0..count)
        .filter(|row| {
            let depth = topology(*row).0;
            if hidden.is_some_and(|parent| depth > parent) {
                return false;
            }
            hidden = closed(*row).then_some(depth);
            true
        })
        .collect()
}
pub(super) fn begin(ui: &Ui, rect: egui::Rect, id: egui::Id, initial: &mut bool) {
    ui.interact(rect, id, egui::Sense::focusable_noninteractive());
    if *initial && ui.is_enabled() {
        ui.memory_mut(|m| m.request_focus(id));
        *initial = false;
        ui.ctx().request_repaint();
    }
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        )
    });
}
pub(super) fn key(ui: &Ui, id: egui::Id) -> Option<egui::Key> {
    if !ui.is_enabled() || !ui.memory(|m| m.has_focus(id)) {
        return None;
    }
    ui.input_mut(|input| {
        [
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::Home,
            egui::Key::End,
            egui::Key::PageDown,
            egui::Key::PageUp,
            egui::Key::Space,
        ]
        .into_iter()
        .find(|key| {
            input.consume_key(egui::Modifiers::NONE, *key)
                || input.consume_key(egui::Modifiers::SHIFT, *key)
        })
    })
}
pub(super) fn navigate(
    key: egui::Key,
    rows: &[usize],
    selected: Option<usize>,
    topology: impl Fn(usize) -> (usize, usize),
    closed: bool,
    page: usize,
) -> Option<Navigation> {
    if rows.is_empty() {
        return None;
    }
    let old = selected.and_then(|s| rows.iter().position(|i| *i == s));
    let current = old.unwrap_or(0);
    let row = rows[current];
    let (depth, end) = topology(row);
    let children = end > row + 1;
    let mut collapsed = None;
    let target = match key {
        egui::Key::ArrowDown => old.map_or(0, |i| (i + 1).min(rows.len() - 1)),
        egui::Key::ArrowUp => current.saturating_sub(1),
        egui::Key::Home => 0,
        egui::Key::End => rows.len() - 1,
        egui::Key::PageDown => (current + page).min(rows.len() - 1),
        egui::Key::PageUp => current.saturating_sub(page),
        egui::Key::ArrowLeft => {
            if children && !closed {
                collapsed = Some(true);
                current
            } else {
                rows[..current]
                    .iter()
                    .rposition(|i| topology(*i).0 < depth)
                    .unwrap_or(current)
            }
        }
        egui::Key::ArrowRight => {
            if children {
                if closed {
                    collapsed = Some(false);
                    current
                } else {
                    current + 1
                }
            } else {
                current
            }
        }
        egui::Key::Space => current,
        _ => return None,
    };
    Some(Navigation {
        row: rows[target],
        collapsed,
    })
}
pub(super) fn scroll(ui: &Ui, salt: impl egui::AsIdSalt, row: usize, height: f32) -> f32 {
    let id = ui.make_persistent_id(egui::IdSalt::new(salt));
    let old = egui::scroll_area::State::load(ui.ctx(), id).map_or(0., |s| s.offset.y);
    old.clamp(((row + 1) as f32 * 30. - height).max(0.), row as f32 * 30.)
}
pub(super) fn selected_color(ui: &Ui, id: egui::Id) -> (Color32, Color32) {
    if ui.memory(|m| m.has_focus(id)) && ui.input(|i| i.focused) {
        (Color32::from_rgb(0, 150, 201), Color32::WHITE)
    } else {
        (Color32::from_gray(211), Color32::BLACK)
    }
}
