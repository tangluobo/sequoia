use super::*;
#[allow(clippy::too_many_arguments)]
pub(super) fn append<'a>(
    rows: &mut Vec<Row<'a>>,
    connection: &'a Connection,
    database: &'a str,
    cache: &'a super::super::backup_tree::Cache,
    path: &'a str,
    depth: usize,
    collapsed: &std::collections::HashSet<String>,
    filter: &str,
    level: usize,
    view: Option<&'a str>,
) {
    if level >= 64 {
        return;
    }
    let key = super::super::backup_tree::Key::new(&connection.id, database, path);
    if filter.is_empty() && collapsed.contains(&key.view_id(view)) {
        return;
    }
    let Some(directory) = cache.directories.get(&key) else {
        return;
    };
    for item in &directory.items {
        let start = rows.len();
        let matches = filter.is_empty() || item.name.to_lowercase().contains(filter);
        rows.push(Row {
            node: Node::Backup(connection, database, path, Some(item), view),
            depth,
        });
        if item.directory {
            let child = key.child(&item.name);
            if (!collapsed.contains(&child.view_id(view)) || !filter.is_empty())
                && let Some((key, _)) = cache.directories.get_key_value(&child)
            {
                append(
                    rows,
                    connection,
                    database,
                    cache,
                    &key.path,
                    depth + 1,
                    collapsed,
                    filter,
                    level + 1,
                    view,
                );
            }
        }
        if !matches && rows.len() == start + 1 {
            rows.pop();
        }
    }
}
