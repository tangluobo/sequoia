use super::*;

/// Input identity is its ordinal, not its legacy ID: two imported entries may
/// carry the same ID and still be independently selected, as in the Java UI.
#[derive(Clone, Copy)]
pub struct Row {
    pub index: usize,
    pub depth: usize,
    pub end: usize,
}

pub(super) fn first_ids(connections: &[Connection]) -> HashMap<&str, usize> {
    let mut ids = HashMap::new();
    for (index, connection) in connections.iter().enumerate() {
        if !connection.id.trim().is_empty() {
            ids.entry(connection.id.as_str()).or_insert(index);
        }
    }
    ids
}

pub(super) fn rows(connections: &[Connection]) -> Result<Vec<Row>> {
    let ids = first_ids(connections);
    let parents: Vec<_> = connections
        .iter()
        .map(|connection| {
            connection
                .parent_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .and_then(|id| ids.get(id).copied())
        })
        .collect();
    let mut states = vec![0u8; parents.len()];
    for start in 0..parents.len() {
        let mut path = Vec::new();
        let mut next = Some(start);
        while let Some(index) = next {
            match states[index] {
                0 => {
                    states[index] = 1;
                    path.push(index);
                    next = parents[index];
                }
                1 => bail!("连接目录存在循环引用"),
                _ => break,
            }
        }
        for index in path {
            states[index] = 2;
        }
    }
    let mut children = vec![Vec::new(); connections.len() + 1];
    for (index, parent) in parents.into_iter().enumerate() {
        children[parent.unwrap_or(connections.len())].push(index);
    }
    let mut pending: Vec<_> = children[connections.len()]
        .iter()
        .rev()
        .map(|index| (*index, 0))
        .collect();
    let mut rows = Vec::<Row>::with_capacity(connections.len());
    let mut stack = Vec::<usize>::new();
    while let Some((index, depth)) = pending.pop() {
        while stack.last().is_some_and(|row| rows[*row].depth >= depth) {
            rows[stack.pop().unwrap()].end = rows.len();
        }
        stack.push(rows.len());
        rows.push(Row {
            index,
            depth,
            end: connections.len(),
        });
        pending.extend(
            children[index]
                .iter()
                .rev()
                .map(|child| (*child, depth + 1)),
        );
    }
    Ok(rows)
}
