use super::*;
use tomato_core::config::export::Tree;
pub(super) fn visible(tree: &Tree<'_>, dialog: &Dialog) -> Vec<usize> {
    picker::visible(
        tree.rows.len(),
        |i| (tree.rows[i].depth, tree.rows[i].end),
        |i| dialog.collapsed.contains(&tree.rows[i].connection.id),
    )
}
pub(super) fn update(
    ui: &Ui,
    id: egui::Id,
    dialog: &mut Dialog,
    tree: &Tree<'_>,
    rows: &mut Vec<usize>,
    page: usize,
) -> Option<usize> {
    let key = picker::key(ui, id)?;
    let selected = tree
        .rows
        .iter()
        .position(|r| Some(&r.connection.id) == dialog.focused.as_ref());
    let closed = selected.is_some_and(|i| dialog.collapsed.contains(&tree.rows[i].connection.id));
    let nav = picker::navigate(
        key,
        rows,
        selected,
        |i| (tree.rows[i].depth, tree.rows[i].end),
        closed,
        page,
    )?;
    let item = tree.rows[nav.row].connection;
    if let Some(closed) = nav.collapsed {
        if closed {
            dialog.collapsed.insert(item.id.clone());
        } else {
            dialog.collapsed.remove(&item.id);
        }
    }
    dialog.focused = Some(item.id.clone());
    *rows = visible(tree, dialog);
    ui.ctx().request_repaint();
    rows.iter().position(|i| *i == nav.row)
}
