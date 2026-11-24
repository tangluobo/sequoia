//! A successful restore invalidates actual database scopes without replacing unsaved drafts.
use super::*;
impl TomatoApp {
    pub(super) fn refresh_restored_database(&mut self, connection: &Connection) {
        use tomato_core::workbench::DatabaseAction;
        let mut jobs = Vec::new();
        let mut schemas = std::collections::BTreeSet::new();
        let oracle_owner = (connection.kind() == "ORACLE")
            .then(|| connection.text("__tomatoOracleOwner"))
            .filter(|s| !s.is_empty());
        for tab in &mut self.tabs {
            let dirty = tab.dirty();
            let Panel::Sql {
                connection: other,
                workbench,
                ..
            } = &mut tab.panel
            else {
                continue;
            };
            if other.id != connection.id
                || tabs::workbench::session_database(other, &workbench.schema)
                    != connection.text("database")
                || oracle_owner.is_some_and(|owner| workbench.schema != owner)
            {
                continue;
            }
            schemas.insert(workbench.schema.clone());
            if workbench.object_load_pending {
                continue;
            }
            if tab.busy || dirty {
                workbench.needs_refresh = true;
                workbench.status = "数据库已还原，请处理未保存内容后刷新".into();
                continue;
            }
            let action = if workbench.browser.is_some() {
                Some(DatabaseAction::Objects {
                    schema: workbench.schema.clone(),
                })
            } else if workbench.designer.is_some() {
                workbench
                    .table
                    .clone()
                    .map(|table| DatabaseAction::DesignStructure { table })
            } else if let Some(mut request) = workbench.request() {
                request.offset = 0;
                request.sort = None;
                Some(DatabaseAction::Page { request })
            } else {
                Some(DatabaseAction::Completion {
                    schema: workbench.schema.clone(),
                })
            };
            if let Some(action) = action {
                jobs.push((tab.id.clone(), other.clone(), action));
            }
        }
        if let Some(host) = self.sql_hosts.get(&connection.id) {
            if let Some(pg) = &host.pg {
                if let Some(database) = pg.databases.get(connection.text("database")) {
                    schemas.extend(database.scopes.keys().cloned());
                }
            } else {
                schemas.extend(
                    host.tables
                        .keys()
                        .filter(|schema| {
                            schema.as_str()
                                == oracle_owner.unwrap_or_else(|| connection.text("database"))
                        })
                        .cloned(),
                );
            }
        }
        for (id, connection, action) in jobs {
            self.submit(
                &id,
                Command::Database { connection, action },
                Purpose::Normal,
            );
        }
        if connection.kind() == "POSTGRESQL" {
            self.reload_pg_namespaces(connection);
        }
        for schema in schemas {
            self.refresh_tree_tables(connection.clone(), schema);
        }
    }
}
