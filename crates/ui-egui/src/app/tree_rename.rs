//! One bounded name editor for all renameable sidebar resources.
use super::*;
use db_tree::{Node, Row};
use std::collections::BTreeSet;
use tomato_core::workbench::TableInfo;
mod controller;
mod field;
#[cfg(test)]
mod server_tests;
#[cfg(test)]
pub(in crate::app) mod tests;

pub(super) type State = Option<Box<Draft>>;
pub(super) enum Target {
    File(Connection, PathBuf),
    Object(Connection, TableInfo),
    Connection(Connection),
    Query {
        connection: Connection,
        namespace: String,
        path: String,
        entry: tomato_core::queries::Entry,
    },
    Backup(backup_tree::Target),
}
pub(super) struct Commit {
    pub target: Target,
    pub name: String,
}
pub(super) struct Draft {
    pub key: String,
    pub name: String,
    pub due: Option<f64>,
    target: Target,
    original: String,
    filter: String,
    focus: bool,
    drawn: bool,
    editor: tabs::text_state::Editor,
}
fn same_connection(a: &Connection, b: &Connection) -> bool {
    a.id == b.id && a.name == b.name && a.kind == b.kind && a.options == b.options
}
pub(super) fn eligible(node: &Node<'_>) -> bool {
    match node {
        Node::LocalFile(_, _)
        | Node::Connection(_)
        | Node::Query(_, _, _, Some(_))
        | Node::Backup(_, _, _, Some(_), _) => true,
        Node::Table(c, _) => matches!(c.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"),
        _ => false,
    }
}
impl Draft {
    pub(super) fn new(row: &Row<'_>, filter: &str, due: Option<f64>) -> Option<Box<Self>> {
        if !eligible(&row.node) {
            return None;
        }
        let target = match &row.node {
            Node::LocalFile(c, e) => Target::File((*c).clone(), e.path.clone()),
            Node::Table(c, t) => Target::Object((*c).clone(), (*t).clone()),
            Node::Connection(c) => Target::Connection((*c).clone()),
            Node::Query(c, n, p, Some(e)) => Target::Query {
                connection: (*c).clone(),
                namespace: (*n).into(),
                path: (*p).into(),
                entry: (*e).clone(),
            },
            Node::Backup(c, d, p, Some(e), n) => {
                Target::Backup(backup_tree::target(c, d, p, Some(e), *n))
            }
            _ => return None,
        };
        Some(Box::new(Self {
            key: row.key(),
            name: row.name().into(),
            original: row.name().into(),
            filter: filter.into(),
            target,
            due,
            focus: due.is_none(),
            drawn: false,
            editor: Default::default(),
        }))
    }
    fn matches(&self, row: &Row<'_>) -> bool {
        if row.key() != self.key || row.name() != self.original {
            return false;
        }
        match (&self.target, &row.node) {
            (Target::Connection(a), Node::Connection(b)) => same_connection(a, b),
            (Target::File(a, p), Node::LocalFile(b, e)) => same_connection(a, b) && p == &e.path,
            (Target::Object(a, t), Node::Table(b, e)) => {
                same_connection(a, b) && t.table == e.table && t.view == e.view
            }
            (
                Target::Query {
                    connection: a,
                    namespace,
                    path,
                    entry,
                },
                Node::Query(b, n, p, Some(e)),
            ) => {
                same_connection(a, b)
                    && namespace == n
                    && path == p
                    && entry.name == e.name
                    && entry.directory == e.directory
            }
            (Target::Backup(a), Node::Backup(b, d, p, Some(e), n)) => {
                same_connection(&a.connection, b)
                    && a.database == *d
                    && a.path == *p
                    && a.namespace.as_deref() == *n
                    && a.item
                        .as_ref()
                        .is_some_and(|i| i.name == e.name && i.directory == e.directory)
            }
            _ => false,
        }
    }
    fn id(&self) -> egui::Id {
        egui::Id::new(("connection-tree-name", &self.key))
    }
    fn release(&self, ctx: &egui::Context) {
        self.editor.release(ctx);
        ctx.memory_mut(|m| m.surrender_focus(self.id()));
    }
}
pub(super) fn editing(state: &State, key: &str) -> bool {
    state
        .as_ref()
        .is_some_and(|d| d.key == key && d.due.is_none())
}
pub(super) fn single(
    row: &Row<'_>,
    selected: Option<&str>,
    connections: &BTreeSet<String>,
    files: &BTreeSet<(String, PathBuf)>,
    tables: &BTreeSet<String>,
) -> bool {
    let key = row.key();
    if selected != Some(key.as_str()) {
        return false;
    }
    match &row.node {
        Node::Connection(c) => connections.len() == 1 && connections.contains(&c.id),
        Node::LocalFile(c, e) => {
            files.len() == 1 && files.iter().any(|(id, p)| id == &c.id && p == &e.path)
        }
        Node::Table(c, _) if matches!(c.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE") => {
            tables.len() == 1 && tables.contains(&key)
        }
        _ => true,
    }
}
pub(super) fn finish(state: &mut State, ctx: &egui::Context, submit: bool) -> Option<Commit> {
    let draft = state.take()?;
    draft.release(ctx);
    let name = draft.name.trim();
    if !submit || draft.due.is_some() || name.is_empty() || name == draft.original {
        return None;
    }
    Some(Commit {
        name: name.into(),
        target: draft.target,
    })
}
pub(super) fn prepare(
    ui: &Ui,
    state: &mut State,
    rows: &[Row<'_>],
    filter: &str,
    selected: Option<&str>,
    allowed: bool,
) {
    let Some(draft) = state.as_ref() else { return };
    let stale = draft.filter != filter || !rows.iter().any(|r| draft.matches(r));
    let pending_invalid = draft.due.is_some()
        && (!allowed
            || selected != Some(draft.key.as_str())
            || egui::Popup::is_any_open(ui.ctx())
            || !ui.input(|i| i.focused)
            || ui.input(|i| i.pointer.is_decidedly_dragging() || i.key_pressed(egui::Key::Escape)));
    if stale || pending_invalid {
        finish(state, ui.ctx(), false);
        return;
    }
    let draft = state.as_mut().unwrap();
    draft.drawn = false;
    if let Some(due) = draft.due {
        let now = ui.input(|i| i.time);
        if now < due {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(due - now));
        } else if !ui.input(|i| i.pointer.any_down()) {
            draft.due = None;
            draft.focus = true;
        }
    }
}
/// Snapshot selection before the click. Only the label, never the icon/disclosure, starts editing.
pub(super) struct Click<'a> {
    pub name_x: f32,
    pub filter: &'a str,
    pub was_single: bool,
    pub allowed: bool,
}
pub(super) fn clicked(
    ui: &Ui,
    state: &mut State,
    row: &Row<'_>,
    response: &egui::Response,
    click: Click<'_>,
) {
    let Click {
        name_x,
        filter,
        was_single,
        allowed,
    } = click;
    if state.as_ref().is_some_and(|d| d.due.is_none()) {
        return;
    }
    if response.double_clicked() || response.triple_clicked() || response.secondary_clicked() {
        finish(state, ui.ctx(), false);
        return;
    }
    if !response.clicked() {
        return;
    }
    finish(state, ui.ctx(), false);
    if !allowed || !was_single || !eligible(&row.node) || !ui.input(|i| i.modifiers.is_none()) {
        return;
    }
    let size = ui
        .painter()
        .layout_no_wrap(
            row.name().into(),
            crate::theme::java_font(12.),
            Color32::BLACK,
        )
        .size();
    let label = egui::Rect::from_min_size(
        egui::pos2(name_x, response.rect.center().y - size.y / 2.),
        size,
    )
    .intersect(response.rect);
    if !ui.input(|i| i.pointer.interact_pos().is_some_and(|p| label.contains(p))) {
        return;
    }
    *state = Draft::new(row, filter, Some(ui.input(|i| i.time) + 0.3));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(300));
}
pub(super) use field::render;
pub(super) fn finish_frame(ui: &Ui, state: &mut State) -> Option<Commit> {
    if state.as_ref().is_some_and(|d| d.due.is_none() && !d.drawn) {
        finish(state, ui.ctx(), true)
    } else {
        None
    }
}
