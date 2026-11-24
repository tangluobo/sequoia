//! Composite-style traversal over a flat persisted model, using one adjacency index.
use crate::Connection;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub enum DropTarget {
    Before(String),
    After(String),
    Into(String),
    Root,
}
/// Validate without copying credentials or rewriting the model while hovering.
pub fn validate_drop(
    connections: &[Connection],
    source: &str,
    target: &DropTarget,
) -> anyhow::Result<Option<String>> {
    use anyhow::{Context, ensure};
    let source_node = connections
        .iter()
        .find(|node| node.id == source)
        .context("连接已不存在")?;
    let parent = match target {
        DropTarget::Root => None,
        DropTarget::Into(id) => {
            let folder = connections
                .iter()
                .find(|node| &node.id == id)
                .context("目标已不存在")?;
            ensure!(folder.kind.is_none(), "目标不是连接分组");
            Some(folder.id.clone())
        }
        DropTarget::Before(id) | DropTarget::After(id) => {
            ensure!(id != source, "不能拖放到自身");
            connections
                .iter()
                .find(|node| &node.id == id)
                .context("目标已不存在")?
                .parent_id
                .clone()
        }
    };
    let descendants = ConnectionTree::new(connections).subtree_ids(&source_node.id);
    ensure!(
        !parent.as_ref().is_some_and(|id| descendants.contains(id)),
        "不能拖放到自身或子分组"
    );
    Ok(parent)
}
pub fn move_connection(
    connections: &[Connection],
    source: &str,
    target: &DropTarget,
) -> anyhow::Result<Vec<Connection>> {
    use anyhow::Context;
    let parent = validate_drop(connections, source, target)?;
    let mut result = connections.to_vec();
    let index = result
        .iter()
        .position(|node| node.id == source)
        .context("连接已不存在")?;
    let mut node = result.remove(index);
    node.parent_id = parent;
    let index = match target {
        DropTarget::Before(id) => result
            .iter()
            .position(|node| &node.id == id)
            .context("目标已不存在")?,
        DropTarget::After(id) => {
            result
                .iter()
                .position(|node| &node.id == id)
                .context("目标已不存在")?
                + 1
        }
        DropTarget::Into(_) | DropTarget::Root => result.len(),
    };
    result.insert(index, node);
    crate::config::validate(&result)?;
    Ok(result)
}

pub struct ConnectionTree<'a> {
    nodes: HashMap<&'a str, &'a Connection>,
    children: HashMap<&'a str, Vec<&'a str>>,
}
impl<'a> ConnectionTree<'a> {
    pub fn new(connections: &'a [Connection]) -> Self {
        let mut nodes = HashMap::new();
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for connection in connections {
            nodes.insert(connection.id.as_str(), connection);
            children
                .entry(connection.parent_id.as_deref().unwrap_or(""))
                .or_default()
                .push(&connection.id);
        }
        Self { nodes, children }
    }
    pub fn subtree_ids(&self, root: &str) -> HashSet<String> {
        let mut result = HashSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !result.insert(id.to_string()) {
                continue;
            }
            if let Some(children) = self.children.get(id) {
                pending.extend(children.iter().copied());
            }
        }
        result
    }
    pub fn rows(&self) -> Vec<(&'a Connection, usize)> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut pending: Vec<_> = self
            .children
            .get("")
            .into_iter()
            .flatten()
            .rev()
            .map(|id| (*id, 0))
            .collect();
        while let Some((id, depth)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(id) {
                result.push((*node, depth));
            }
            if let Some(children) = self.children.get(id) {
                pending.extend(children.iter().rev().map(|id| (*id, depth + 1)));
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_preserve_subtrees_unknown_fields_and_reject_cycles() -> anyhow::Result<()> {
        let mut group = Connection::new("SSH");
        group.kind = None;
        let mut nested = Connection::new("SSH");
        nested.kind = None;
        nested.parent_id = Some(group.id.clone());
        let mut child = Connection::new("SSH");
        child.parent_id = Some(nested.id.clone());
        child.set("futureOption", "preserved");
        child.set("password", "fixture-only");
        let other = Connection::new("MYSQL");
        let nodes = vec![group.clone(), nested.clone(), child.clone(), other.clone()];
        assert!(move_connection(&nodes, &group.id, &DropTarget::Into(nested.id.clone())).is_err());
        assert!(move_connection(&nodes, &child.id, &DropTarget::Into(other.id.clone())).is_err());
        let moved = move_connection(&nodes, &nested.id, &DropTarget::Root)?;
        assert!(moved.last().unwrap().parent_id.is_none());
        assert_eq!(
            moved
                .iter()
                .find(|node| node.id == child.id)
                .unwrap()
                .parent_id
                .as_deref(),
            Some(nested.id.as_str())
        );
        let moved = move_connection(&moved, &other.id, &DropTarget::Before(group.id.clone()))?;
        assert_eq!(ConnectionTree::new(&moved).rows()[0].0.id, other.id);
        let retained = moved.iter().find(|node| node.id == child.id).unwrap();
        assert_eq!(retained.text("futureOption"), "preserved");
        assert_eq!(retained.text("password"), "fixture-only");
        Ok(())
    }
    #[test]
    fn traversal_orders_children_and_survives_cycles() {
        let mut folder = Connection::new("SSH");
        folder.kind = None;
        let mut child = Connection::new("MYSQL");
        child.parent_id = Some(folder.id.clone());
        let items = vec![child.clone(), folder.clone()];
        let tree = ConnectionTree::new(&items);
        assert_eq!(
            tree.rows()
                .iter()
                .map(|(_, depth)| *depth)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(tree.subtree_ids(&folder.id).len(), 2);
        folder.parent_id = Some(child.id.clone());
        let cyclic = vec![folder.clone(), child];
        assert_eq!(
            ConnectionTree::new(&cyclic).subtree_ids(&folder.id).len(),
            2
        );
    }
}
