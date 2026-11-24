//! PostgreSQL sidebar requests own actual-database scope, independently of query tabs.
use super::super::*;
use std::{collections::BTreeMap, sync::Arc};
use tomato_core::workbench::{DatabaseAction, DatabaseOutput, TableInfo};
#[derive(Default)]
pub(in crate::app) struct Tree {
    pub names: Arc<Vec<String>>,
    pub databases: BTreeMap<String, Database>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    serial: u64,
    generation: u64,
}
pub(in crate::app) struct Database {
    pub connection: Connection,
    pub namespaces: Option<Arc<Vec<String>>>,
    pub scopes: BTreeMap<String, Namespace>,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
}
pub(in crate::app) struct Namespace {
    pub query_key: String,
    pub tables: Option<Arc<Vec<TableInfo>>>,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
}
pub(in crate::app) struct Job {
    pub connection: String,
    epoch: String,
    generation: u64,
    kind: Kind,
}
enum Kind {
    CloseDatabase,
    Databases(bool),
    Schemas(String),
    Tables(String, String),
}
pub(in crate::app) fn database_key(id: &str, name: &str) -> String {
    format!(
        "pg-database:{id}:{}",
        serde_json::to_string(name).unwrap_or_default()
    )
}
pub(in crate::app) fn schema_key(connection: &Connection, schema: &str) -> String {
    if connection.kind() == "POSTGRESQL" {
        format!(
            "pg-schema:{}:{}",
            connection.id,
            serde_json::to_string(&(connection.text("database"), schema)).unwrap_or_default()
        )
    } else {
        format!("schema:{}:{schema}", connection.id)
    }
}
impl Tree {
    fn retained_bytes(&self) -> usize {
        let mut bytes = names_bytes(&self.names);
        for (name, db) in &self.databases {
            bytes = bytes.saturating_add(
                name.capacity() + 256 + db.namespaces.as_ref().map_or(0, |s| names_bytes(s)),
            );
            for (name, scope) in &db.scopes {
                bytes = bytes.saturating_add(
                    name.capacity()
                        + scope.query_key.capacity()
                        + 128
                        + scope.tables.as_ref().map_or(0, |t| tables_bytes(t)),
                );
            }
        }
        bytes
    }
    pub fn matches(&self, filter: &str) -> bool {
        self.names
            .iter()
            .any(|name| name.to_lowercase().contains(filter))
            || self.databases.values().any(|db| {
                db.namespaces
                    .iter()
                    .flat_map(|s| s.iter())
                    .any(|s| s.to_lowercase().contains(filter))
                    || db.scopes.values().any(|scope| {
                        scope
                            .tables
                            .iter()
                            .flat_map(|t| t.iter())
                            .any(|t| t.table.name.to_lowercase().contains(filter))
                    })
            })
    }
}
impl TomatoApp {
    pub(in crate::app) fn reload_pg_namespaces(&mut self, connection: &Connection) {
        let Some(host) = self.sql_hosts.get_mut(&connection.id) else {
            return;
        };
        let Some(pg) = &mut host.pg else { return };
        let Some(db) = pg.databases.get_mut(connection.text("database")) else {
            return;
        };
        pg.serial += 1;
        db.generation = pg.serial;
        db.loading = true;
        db.error = None;
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: pg.serial,
            kind: Kind::Schemas(connection.text("database").into()),
        };
        self.submit_pg_tree(connection.clone(), DatabaseAction::Schemas, job);
    }
    pub(in crate::app) fn close_pg_database(&mut self, connection: Connection, database: String) {
        // Java closes structural/data tabs, but independent query editors keep their session.
        let same_scope = |tab: &&Tab| matches!(&tab.panel, Panel::Sql { connection: c, .. } if c.id == connection.id && c.text("database") == database);
        let kept_query = self.tabs.iter().filter(same_scope).any(|tab| matches!(&tab.panel, Panel::Sql { workbench, .. } if workbench.browser.is_none() && workbench.table.is_none() && workbench.designer.is_none()));
        let tabs = self.tabs.iter().filter(same_scope).filter(|tab| matches!(&tab.panel, Panel::Sql { workbench, .. } if workbench.browser.is_some() || workbench.table.is_some() || workbench.designer.is_some())).collect::<Vec<_>>();
        if tabs.iter().any(|tab| tab.dirty()) {
            self.error = Some("此数据库还有未保存的内容，请先保存或取消修改后关闭数据库".into());
            return;
        }
        let ids = tabs.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>();
        let Some(host) = self.sql_hosts.get(&connection.id) else {
            return;
        };
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: 0,
            kind: Kind::CloseDatabase,
        };
        if !kept_query {
            match self.queue.submit(Command::DisconnectDatabase {
                id: connection.id.clone(),
                database: database.clone(),
            }) {
                Ok(task) => {
                    self.pending.insert(
                        task,
                        Pending {
                            tab_id: connection.id.clone(),
                            purpose: Purpose::PgTree(job),
                        },
                    );
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    return;
                }
            }
        }
        for id in ids {
            self.close_tab(&id, false);
        }
        if let Some(pg) = self
            .sql_hosts
            .get_mut(&connection.id)
            .and_then(|host| host.pg.as_mut())
        {
            pg.databases.remove(&database);
        }
        if !kept_query && let Some(host) = self.sql_hosts.get_mut(&connection.id) {
            host.transactions.remove(&database);
        }
        self.collapsed
            .insert(database_key(&connection.id, &database));
    }
    pub(in crate::app) fn open_pg_host(&mut self, connection: Connection, refresh: bool) {
        self.collapsed.remove(&connection.id);
        let host = self
            .sql_hosts
            .entry(connection.id.clone())
            .or_insert_with(|| sql_tree::Host::new(connection.clone()));
        if host
            .pg
            .as_ref()
            .is_some_and(|pg| !refresh && (pg.loading || pg.loaded))
        {
            return;
        }
        let pg = host.pg.get_or_insert_with(Tree::default);
        pg.serial += 1;
        pg.generation = pg.serial;
        pg.loading = true;
        pg.error = None;
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: pg.generation,
            kind: Kind::Databases(refresh),
        };
        self.submit_pg_tree(connection, DatabaseAction::Databases, job);
    }
    pub(in crate::app) fn open_pg_database(
        &mut self,
        connection: Connection,
        database: String,
        refresh: bool,
    ) {
        let Some(host) = self.sql_hosts.get_mut(&connection.id) else {
            return;
        };
        let Some(pg) = &mut host.pg else { return };
        if !pg.databases.contains_key(&database) && pg.databases.len() >= 32 {
            self.error = Some("已打开32个数据库，请先关闭不使用的数据库".into());
            return;
        }
        self.collapsed
            .remove(&database_key(&connection.id, &database));
        if pg
            .databases
            .get(&database)
            .is_some_and(|db| !refresh && (db.loading || db.namespaces.is_some()))
        {
            return;
        }
        pg.serial += 1;
        let db = pg.databases.entry(database.clone()).or_insert_with(|| {
            let mut routed = connection.clone();
            routed.set("database", database.clone());
            Database {
                connection: routed,
                namespaces: None,
                scopes: BTreeMap::new(),
                loading: false,
                error: None,
                generation: 0,
            }
        });
        if refresh {
            db.scopes.clear();
        }
        db.generation = pg.serial;
        db.loading = true;
        db.error = None;
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: pg.serial,
            kind: Kind::Schemas(database),
        };
        let routed = db.connection.clone();
        self.submit_pg_tree(routed, DatabaseAction::Schemas, job);
    }
    pub(in crate::app) fn open_pg_schema(
        &mut self,
        connection: Connection,
        schema: String,
        refresh: bool,
    ) {
        let Some(host) = self.sql_hosts.get_mut(&connection.id) else {
            return;
        };
        let Some(pg) = &mut host.pg else { return };
        if pg
            .databases
            .values()
            .map(|db| db.scopes.len())
            .sum::<usize>()
            >= 256
            && !pg
                .databases
                .get(connection.text("database"))
                .is_some_and(|db| db.scopes.contains_key(&schema))
        {
            self.error = Some("已打开256个模式，请先关闭不使用的数据库".into());
            return;
        }
        let Some(db) = pg.databases.get_mut(connection.text("database")) else {
            return;
        };
        self.collapsed.remove(&schema_key(&connection, &schema));
        if db
            .scopes
            .get(&schema)
            .is_some_and(|s| !refresh && (s.loading || s.tables.is_some()))
        {
            return;
        }
        if refresh || !db.scopes.contains_key(&schema) {
            self.collapsed.insert(
                backup_tree::Key::new(&connection.id, connection.text("database"), "")
                    .view_id(Some(&schema)),
            );
            self.collapsed.insert(query_tree::key(
                &connection.id,
                &query_tree::identity::key(&connection, &schema),
                "",
                None,
            ));
        }
        pg.serial += 1;
        let scope = db
            .scopes
            .entry(schema.clone())
            .or_insert_with(|| Namespace {
                query_key: query_tree::identity::key(&connection, &schema),
                tables: None,
                loading: false,
                error: None,
                generation: 0,
            });
        if refresh {
            scope.tables = None;
        }
        scope.generation = pg.serial;
        scope.loading = true;
        scope.error = None;
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: pg.serial,
            kind: Kind::Tables(connection.text("database").into(), schema.clone()),
        };
        self.submit_pg_tree(connection, DatabaseAction::Tables { schema }, job);
    }
    fn submit_pg_tree(&mut self, connection: Connection, action: DatabaseAction, job: Job) {
        match self.queue.submit(Command::Database { connection, action }) {
            Ok(id) => {
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: job.connection.clone(),
                        purpose: Purpose::PgTree(job),
                    },
                );
            }
            Err(e) => self.pg_tree_reply(job, Err(e)),
        }
    }
    pub(in crate::app) fn pg_tree_reply(&mut self, job: Job, result: anyhow::Result<Output>) {
        let Some(host) = self
            .sql_hosts
            .get_mut(&job.connection)
            .filter(|h| h.epoch == job.epoch)
        else {
            return;
        };
        let Some(pg) = &mut host.pg else { return };
        if matches!(job.kind, Kind::CloseDatabase) {
            if let Err(error) = result {
                self.error = Some(format!("关闭数据库失败: {error}"));
            }
            return;
        }
        let retained = if matches!(&job.kind, Kind::Databases(true)) {
            0
        } else {
            pg.retained_bytes()
        };
        let result = result.and_then(|output| {
            if let Output::Database(value) = output {
                let incoming = match &value {
                    DatabaseOutput::Databases(names) | DatabaseOutput::Schemas(names) => {
                        names_bytes(names)
                    }
                    DatabaseOutput::Tables(tables) => tables_bytes(tables),
                    _ => 0,
                };
                anyhow::ensure!(
                    retained.saturating_add(incoming) <= 16 * 1024 * 1024,
                    "PostgreSQL树缓存超过16 MiB，请先关闭不使用的数据库"
                );
                Ok(value)
            } else {
                anyhow::bail!("数据库树任务返回了错误类型")
            }
        });
        let error = match job.kind {
            Kind::Databases(reset) if pg.generation == job.generation => {
                pg.loading = false;
                match result {
                    Ok(DatabaseOutput::Databases(names)) => {
                        pg.loaded = true;
                        if reset {
                            pg.databases.clear();
                        } else {
                            pg.databases.retain(|db, _| names.contains(db));
                        }
                        pg.names = Arc::new(names);
                        pg.error = None;
                        None
                    }
                    Err(e) => {
                        let error = e.to_string();
                        pg.error = Some(error.clone());
                        Some(error)
                    }
                    _ => Some("数据库目录返回类型错误".into()),
                }
            }
            Kind::Schemas(database) => {
                let Some(db) = pg
                    .databases
                    .get_mut(&database)
                    .filter(|db| db.generation == job.generation)
                else {
                    return;
                };
                db.loading = false;
                match result {
                    Ok(DatabaseOutput::Schemas(names)) => {
                        db.scopes.retain(|s, _| names.contains(s));
                        db.namespaces = Some(Arc::new(names));
                        db.error = None;
                        None
                    }
                    Err(e) => {
                        let error = e.to_string();
                        db.error = Some(error.clone());
                        Some(error)
                    }
                    _ => Some("模式目录返回类型错误".into()),
                }
            }
            Kind::Tables(database, schema) => {
                let Some(db) = pg.databases.get_mut(&database) else {
                    return;
                };
                let Some(scope) = db
                    .scopes
                    .get_mut(&schema)
                    .filter(|s| s.generation == job.generation)
                else {
                    return;
                };
                scope.loading = false;
                match result {
                    Ok(DatabaseOutput::Tables(tables)) => {
                        if scope.tables.is_none() {
                            for folder in [db_tree::Folder::Tables, db_tree::Folder::Views] {
                                self.collapsed.insert(folder.key(&db.connection, &schema));
                            }
                        }
                        scope.tables = Some(Arc::new(tables));
                        scope.error = None;
                        None
                    }
                    Err(e) => {
                        let error = e.to_string();
                        scope.error = Some(error.clone());
                        Some(error)
                    }
                    _ => Some("对象目录返回类型错误".into()),
                }
            }
            _ => return,
        };
        if let Some(error) = error {
            self.error = Some(error);
        }
    }
}

pub(super) fn names_bytes(names: &Vec<String>) -> usize {
    names.capacity() * std::mem::size_of::<String>()
        + names.iter().map(String::capacity).sum::<usize>()
}
pub(super) fn tables_bytes(tables: &Vec<TableInfo>) -> usize {
    tables.capacity() * std::mem::size_of::<TableInfo>()
        + tables
            .iter()
            .map(|t| t.table.schema.capacity() + t.table.name.capacity())
            .sum::<usize>()
}
#[cfg(test)]
pub(in crate::app) mod tests;
