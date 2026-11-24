//! Visible tree traversal with scoped keyboard focus and the existing typed selections.
use super::{
    db_tree::{Node, Row},
    *,
};
#[cfg(test)]
mod tests;
pub(super) fn select(
    app: &mut Selection<'_>,
    rows: &[Row<'_>],
    index: usize,
    modifiers: egui::Modifiers,
) {
    let row = &rows[index];
    let key = row.key();
    let anchor = app
        .anchor
        .as_ref()
        .and_then(|key| rows.iter().position(|r| r.key() == *key))
        .unwrap_or(index);
    if !modifiers.command {
        app.connections.clear();
        app.files.clear();
        app.sql.clear();
    }
    let range = if modifiers.shift {
        anchor.min(index)..=anchor.max(index)
    } else {
        index..=index
    };
    for row in &rows[range] {
        match &row.node {
            Node::Connection(c) => {
                app.connections.insert(c.id.clone());
            }
            Node::LocalFile(c, e) => {
                app.files.insert((c.id.clone(), e.path.clone()));
            }
            Node::Table(c, _) if matches!(c.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE") => {
                app.sql.insert(row.key());
            }
            _ => {}
        }
    }
    if !modifiers.shift {
        *app.anchor = Some(key.clone());
    }
    *app.selected = Some(key);
}
pub(super) struct Selection<'a> {
    pub selected: &'a mut Option<String>,
    pub anchor: &'a mut Option<String>,
    pub connections: &'a mut std::collections::BTreeSet<String>,
    pub files: &'a mut std::collections::BTreeSet<(String, PathBuf)>,
    pub sql: &'a mut std::collections::BTreeSet<String>,
}
pub(super) fn next(
    ui: &Ui,
    rows: &[Row<'_>],
    selected: Option<&str>,
    page: usize,
) -> Option<usize> {
    if !ui.input(|i| {
        [
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::PageUp,
            egui::Key::PageDown,
            egui::Key::Home,
            egui::Key::End,
        ]
        .iter()
        .any(|key| i.key_pressed(*key))
    }) || rows.is_empty()
    {
        return None;
    }
    let current = selected.and_then(|key| rows.iter().position(|r| r.key() == key));
    let modifiers = ui.input(|i| i.modifiers);
    for (key, delta) in [
        (egui::Key::ArrowUp, -1),
        (egui::Key::ArrowDown, 1),
        (egui::Key::PageUp, -(page as isize)),
        (egui::Key::PageDown, page as isize),
        (egui::Key::Home, isize::MIN),
        (egui::Key::End, isize::MAX),
    ] {
        if ui.input_mut(|i| i.consume_key(modifiers, key)) {
            return Some(match delta {
                isize::MIN => 0,
                isize::MAX => rows.len() - 1,
                _ => current
                    .map(|i| i.saturating_add_signed(delta).min(rows.len() - 1))
                    .unwrap_or(0),
            });
        }
    }
    if ui.input(|i| i.key_pressed(egui::Key::ArrowRight))
        && let Some(current) = current
        && rows
            .get(current + 1)
            .is_some_and(|row| row.depth > rows[current].depth)
    {
        return Some(current + 1);
    }
    if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft))
        && let Some(current) = current
        && !rows
            .get(current + 1)
            .is_some_and(|row| row.depth > rows[current].depth)
    {
        return (0..current)
            .rev()
            .find(|i| rows[*i].depth < rows[current].depth);
    }
    None
}
