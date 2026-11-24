//! Toolbar selection owns the editor scope; completed results keep their own scope.
use super::*;
impl TomatoApp {
    pub(super) fn choose_sql_connection(&mut self, id: &str, name: &str) {
        let target = self
            .store
            .connections
            .iter()
            .find(|c| {
                c.name == name
                    && matches!(
                        c.kind(),
                        "MYSQL" | "POSTGRESQL" | "SQLITE" | "ORACLE" | "HIVE"
                    )
            })
            .cloned();
        if let Some(connection) = target {
            let connection = self.live_sql_connection(connection);
            if matches!(
                connection.kind(),
                "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
            ) && self.sql_password_missing(&connection)
            {
                self.credential = Some(credentials::Prompt::then(
                    connection,
                    credentials::Resume::SqlConnection { tab: id.into() },
                ));
                return;
            }
            self.apply_sql_connection(id, connection);
        } else if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.error = Some("请选择数据库连接".into());
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                workbench.selectors.identity = None;
            }
        }
    }
    pub(super) fn apply_sql_connection(&mut self, id: &str, connection: Connection) {
        if !self.tabs.iter().any(|tab| {
            tab.id == id
                && matches!(&tab.panel,Panel::Sql{workbench,..}if workbench.table.is_none())
        }) {
            return;
        }
        let preferred = self.tabs.iter().find(|tab| tab.id == id).and_then(|tab| {
            if let Panel::Sql {
                connection,
                workbench,
                ..
            } = &tab.panel
            {
                Some(
                    crate::tabs::workbench::query_database(connection, &workbench.schema)
                        .to_owned(),
                )
            } else {
                None
            }
        });
        self.replace_sql_scope(id, connection.clone(), false, None);
        self.fetch_sql_databases(id, connection, preferred);
    }
    pub(super) fn choose_sql_database(&mut self, id: &str, name: &str) {
        let name = name.trim_matches(|ch: char| ch <= ' ');
        let connection = self.tabs.iter().find(|tab| tab.id == id).and_then(|tab| {
            if let Panel::Sql { connection, .. } = &tab.panel {
                Some(connection.clone())
            } else {
                None
            }
        });
        if let Some(mut connection) = connection {
            if name.is_empty() {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.error = Some("请先选择数据库".into());
                    if let Panel::Sql { workbench, .. } = &mut tab.panel {
                        workbench.selectors.identity = None;
                    }
                }
                return;
            }
            let namespace = if matches!(connection.kind(), "SQLITE" | "ORACLE") {
                Some(name.to_owned())
            } else {
                connection = tomato_core::database_scope::select(&connection, name);
                None
            };
            self.replace_sql_scope(id, connection, true, namespace);
        }
    }
    fn replace_sql_scope(
        &mut self,
        id: &str,
        target: Connection,
        load_schema: bool,
        selected_namespace: Option<String>,
    ) {
        let transaction = self.sql_transaction(
            &target,
            if target.kind() == "SQLITE" {
                "main"
            } else {
                target.text("database")
            },
        );
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        if workbench.table.is_some() {
            return;
        }
        let namespace = selected_namespace.unwrap_or_else(|| {
            match target.kind() {
                "MYSQL" => target.text("database"),
                "HIVE" => tomato_core::database_scope::hive_name(target.text("database")),
                "POSTGRESQL"
                    if connection.kind() == "POSTGRESQL" && !workbench.schema.is_empty() =>
                {
                    workbench.schema.as_str()
                }
                "POSTGRESQL" => "public",
                "ORACLE" => target.text("username"),
                _ => "main",
            }
            .to_owned()
        });
        if let Some(document) = &mut workbench.document {
            let scope = tomato_core::queries::Scope {
                connection: target.name.clone(),
                database: crate::tabs::workbench::query_database(&target, &namespace).into(),
                path: document.scope.path.clone(),
            };
            if document.scope != scope {
                document.generation = document.generation.wrapping_add(1);
                document.scope = scope;
                document.stamp = None;
                document.loading = false;
                document.saving = false;
                document.close_after_save = false;
                document.resave = false;
            }
            document.namespace = super::query_tree::identity::key(&target, &namespace);
        }
        *connection = target.clone();
        workbench.transaction = transaction;
        workbench.schema = namespace;
        workbench.schemas = Default::default();
        workbench.tables = Default::default();
        workbench.selectors.selected = true;
        workbench.selectors.identity = None;
        workbench.completion.generation = workbench.completion.generation.wrapping_add(1);
        workbench.completion.metadata = None;
        workbench.completion.error.clear();
        workbench.completion.invalidate();
        self.sql_hosts
            .entry(target.id.clone())
            .or_insert_with(|| sql_tree::Host::new(target.clone()));
        workbench.execution = None;
        if target.kind() == "POSTGRESQL" {
            self.open_pg_host(target.clone(), false);
        } else if matches!(target.kind(), "MYSQL" | "ORACLE" | "HIVE") {
            self.open_direct_host(target.clone(), false);
        }
        if load_schema {
            self.submit(
                id,
                Command::Database {
                    connection: target,
                    action: tomato_core::workbench::DatabaseAction::Schemas,
                },
                Purpose::Normal,
            );
        }
    }

    fn fetch_sql_databases(&mut self, id: &str, connection: Connection, preferred: Option<String>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            return;
        };
        workbench.selectors.catalog_generation =
            workbench.selectors.catalog_generation.wrapping_add(1);
        workbench.selectors.databases = Default::default();
        let purpose = Purpose::SqlDatabases {
            connection: connection.id.clone(),
            generation: workbench.selectors.catalog_generation,
            preferred,
        };
        match self.queue.submit(Command::Database {
            connection,
            action: tomato_core::workbench::DatabaseAction::Databases,
        }) {
            Ok(task) => {
                self.pending.retain(|_, pending| {
                    pending.tab_id != id
                        || matches!(
                            pending.purpose,
                            Purpose::SqlRequest(_)
                                | Purpose::QuerySave { .. }
                                | Purpose::AiSql(_)
                                | Purpose::SqlCompletion(_)
                        )
                });
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.busy = self.pending.values().any(|pending| {
                        pending.tab_id == id
                            && matches!(
                                pending.purpose,
                                Purpose::QuerySave { .. } | Purpose::SqlRequest(_)
                            )
                    });
                }
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: id.into(),
                        purpose,
                    },
                );
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}
