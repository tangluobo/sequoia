use super::*;
#[allow(clippy::too_many_arguments)]
pub(super) fn append<'a>(
    rows: &mut Vec<Row<'a>>,
    host: &'a super::super::sql_tree::Host,
    queries: &'a super::super::query_tree::Controller,
    backups: &'a super::super::backup_tree::Cache,
    collapsed: &std::collections::HashSet<String>,
    filter: &str,
    depth: usize,
    parent_matches: bool,
) {
    let pg = host.pg.as_ref().unwrap();
    for name in pg.names.iter() {
        let start = rows.len();
        let db = pg.databases.get(name);
        let matches = parent_matches || name.to_lowercase().contains(filter);
        rows.push(Row {
            node: Node::Database(&host.connection, name, db.is_some()),
            depth: depth + 1,
        });
        if (!filter.is_empty()
            || !collapsed.contains(&super::super::sql_tree::postgres::database_key(
                &host.connection.id,
                name,
            )))
            && let Some(db) = db
            && let Some(namespaces) = &db.namespaces
        {
            for namespace in namespaces.iter() {
                let schema_start = rows.len();
                let scope = db.scopes.get(namespace);
                let schema_matches = matches || namespace.to_lowercase().contains(filter);
                rows.push(Row {
                    node: Node::Schema(&db.connection, namespace, scope.is_some()),
                    depth: depth + 2,
                });
                if (!filter.is_empty()
                    || !collapsed.contains(&super::super::sql_tree::postgres::schema_key(
                        &db.connection,
                        namespace,
                    )))
                    && let Some(scope) = scope
                {
                    for folder in [Folder::Tables, Folder::Views, Folder::Functions] {
                        let folder_start = rows.len();
                        let folder_matches = schema_matches || folder.label().contains(filter);
                        rows.push(Row {
                            node: Node::Objects(&db.connection, namespace, folder),
                            depth: depth + 3,
                        });
                        if folder != Folder::Functions
                            && (!filter.is_empty()
                                || !collapsed.contains(&folder.key(&db.connection, namespace)))
                        {
                            for table in scope
                                .tables
                                .iter()
                                .flat_map(|t| t.iter())
                                .filter(|t| t.view == (folder == Folder::Views))
                            {
                                if folder_matches
                                    || table.table.name.to_lowercase().contains(filter)
                                {
                                    rows.push(Row {
                                        node: Node::Table(&db.connection, table),
                                        depth: depth + 4,
                                    });
                                }
                            }
                        }
                        if !folder_matches && rows.len() == folder_start + 1 {
                            rows.pop();
                        }
                    }
                    if let Some(tree) = queries.tree(&db.connection.id, &scope.query_key) {
                        let at = rows.len();
                        rows.push(Row {
                            node: Node::Query(&db.connection, &scope.query_key, "", None),
                            depth: depth + 3,
                        });
                        append_queries(
                            rows,
                            &db.connection,
                            &scope.query_key,
                            tree,
                            "",
                            depth + 4,
                            collapsed,
                            if schema_matches { "" } else { filter },
                            0,
                        );
                        if !schema_matches && !"查询".contains(filter) && rows.len() == at + 1 {
                            rows.pop();
                        }
                    } else if schema_matches {
                        rows.push(Row {
                            node: Node::Query(&db.connection, &scope.query_key, "", None),
                            depth: depth + 3,
                        });
                    }
                    let at = rows.len();
                    rows.push(Row {
                        node: Node::Backup(&db.connection, name, "", None, Some(namespace)),
                        depth: depth + 3,
                    });
                    super::backups::append(
                        rows,
                        &db.connection,
                        name,
                        backups,
                        "",
                        depth + 4,
                        collapsed,
                        if schema_matches { "" } else { filter },
                        0,
                        Some(namespace),
                    );
                    if !schema_matches && !"备份".contains(filter) && rows.len() == at + 1 {
                        rows.pop();
                    }
                }
                if !schema_matches && rows.len() == schema_start + 1 {
                    rows.pop();
                }
            }
        }
        if !matches && rows.len() == start + 1 {
            rows.pop();
        }
    }
}
