//! Sidebar metadata has its own lifetime; closing a query cannot rewrite another editor's scope.
use super::*;
use std::{collections::HashSet, sync::Arc};
use tomato_core::workbench::TableInfo;
pub(super) mod direct;
#[cfg(test)]
mod lazy_tests;
pub(super) mod postgres;
pub(super) struct Host {
    pub epoch: String,
    pub connection: Connection,
    pub schemas: Arc<Vec<String>>,
    pub tables: HashMap<String, Arc<Vec<TableInfo>>>,
    pub selected: String,
    pub pg: Option<postgres::Tree>,
    pub direct: Option<direct::Tree>,
    pub transactions: HashSet<String>,
}
impl Host {
    pub fn tables_for_mut(
        &mut self,
        connection: &Connection,
        schema: &str,
    ) -> Option<&mut Arc<Vec<TableInfo>>> {
        if let Some(pg) = &mut self.pg {
            return pg
                .databases
                .get_mut(connection.text("database"))?
                .scopes
                .get_mut(schema)?
                .tables
                .as_mut();
        }
        self.tables.get_mut(schema)
    }
    pub fn replace_tables(
        &mut self,
        connection: &Connection,
        schema: String,
        tables: Arc<Vec<TableInfo>>,
    ) {
        if let Some(pg) = &mut self.pg {
            if let Some(scope) = pg
                .databases
                .get_mut(connection.text("database"))
                .and_then(|db| db.scopes.get_mut(&schema))
            {
                scope.tables = Some(tables);
            }
            return;
        }
        if matches!(connection.kind(), "MYSQL" | "HIVE")
            || self.connection.text("database") == connection.text("database")
        {
            self.tables.insert(schema, tables);
        }
    }
    pub fn new(connection: Connection) -> Self {
        Self {
            epoch: Connection::new("TOOL").id,
            connection,
            schemas: Arc::new(Vec::new()),
            tables: HashMap::new(),
            selected: String::new(),
            pg: None,
            direct: None,
            transactions: HashSet::new(),
        }
    }
}
impl Host {
    pub fn tables_for(
        &self,
        connection: &Connection,
        schema: &str,
    ) -> Option<&Arc<Vec<TableInfo>>> {
        if let Some(pg) = &self.pg {
            return pg
                .databases
                .get(connection.text("database"))?
                .scopes
                .get(schema)?
                .tables
                .as_ref();
        }
        self.tables.get(schema)
    }
}

impl TomatoApp {
    pub(super) fn refresh_tree_tables(&mut self, connection: Connection, schema: String) {
        let Some(host) = self.sql_hosts.get(&connection.id) else {
            return;
        };
        if host.pg.is_some() {
            self.open_pg_schema(connection, schema, true);
            return;
        }
        if host.direct.is_some() {
            self.refresh_open_direct_schema(connection, schema);
            return;
        }
        self.reload_tree_tables(connection, schema, false);
    }
    pub(super) fn reload_tree_names(&mut self, connection: Connection, schema: String) {
        self.reload_tree_tables(connection, schema, true);
    }
    fn reload_tree_tables(&mut self, connection: Connection, schema: String, existing_only: bool) {
        let Some(host) = self.sql_hosts.get(&connection.id) else {
            return;
        };
        let expected_tables = existing_only
            .then(|| host.tables_for(&connection, &schema).cloned())
            .flatten();
        if existing_only && expected_tables.is_none() {
            return;
        }
        let epoch = host.epoch.clone();
        self.pending.retain(|_,pending|!matches!(&pending.purpose,Purpose::TreeTables{connection:c,schema:s,..}if c.id==connection.id&&c.text("database")==connection.text("database")&&s==&schema));
        match self.queue.submit(Command::Database {
            connection: connection.clone(),
            action: tomato_core::workbench::DatabaseAction::Tables {
                schema: schema.clone(),
            },
        }) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: connection.id.clone(),
                        purpose: Purpose::TreeTables {
                            connection,
                            schema,
                            epoch,
                            expected_tables,
                        },
                    },
                );
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

impl TomatoApp {
    /// A non-remembered password belongs to the live host, never to the persisted file.
    pub(super) fn live_sql_connection(&self, connection: Connection) -> Connection {
        let Some(host) = self.sql_hosts.get(&connection.id) else {
            return connection;
        };
        let mut live = host.connection.clone();
        live.set("database", connection.text("database"));
        live
    }
}

impl TomatoApp {
    pub(super) fn sql_transaction(&self, connection: &Connection, schema: &str) -> bool {
        let database = tabs::workbench::session_database(connection, schema);
        self.sql_hosts.get(&connection.id).is_some_and(|host| host.transactions.contains(database)) || self.tabs.iter().any(|tab| matches!(&tab.panel, Panel::Sql { connection: other, workbench, .. } if other.id == connection.id && tabs::workbench::session_database(other, &workbench.schema) == database && workbench.transaction))
    }
}

impl TomatoApp {
    pub(super) fn refresh_sql_host(&mut self, connection: Connection) {
        let connection = self.live_sql_connection(connection);
        if self.sql_password_missing(&connection) {
            self.credential = Some(credentials::Prompt::new(connection));
            return;
        }
        match connection.kind() {
            "MYSQL" | "ORACLE" | "HIVE" => self.open_direct_host(connection, true),
            "POSTGRESQL" => self.open_pg_host(connection, true),
            _ => {}
        }
    }
}
