//! Flat owner/database tree shared by MySQL, Oracle and Hive. PostgreSQL has an extra schema level.
use super::super::*;
use std::collections::HashMap;
use tomato_core::workbench::{DatabaseAction, DatabaseOutput};
#[derive(Default)]
pub(in crate::app) struct Tree {
    pub loading: bool,
    pub loaded: bool,
    pub scopes: HashMap<String, Scope>,
    serial: u64,
    generation: u64,
}
pub(in crate::app) struct Scope {
    pub loading: bool,
    generation: u64,
}
pub(in crate::app) struct Job {
    connection: String,
    epoch: String,
    generation: u64,
    kind: Kind,
}
enum Kind {
    Host(bool),
    Tables(String),
    Close,
}
impl TomatoApp {
    pub(in crate::app) fn open_direct_host(&mut self, connection: Connection, refresh: bool) {
        self.collapsed.remove(&connection.id);
        let host = self
            .sql_hosts
            .entry(connection.id.clone())
            .or_insert_with(|| sql_tree::Host::new(connection.clone()));
        let tree = host.direct.get_or_insert_with(Tree::default);
        if !refresh && (tree.loading || tree.loaded) {
            return;
        }
        tree.serial = tree.serial.wrapping_add(1);
        tree.generation = tree.serial;
        tree.loading = true;
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: tree.generation,
            kind: Kind::Host(refresh),
        };
        self.submit_direct_tree(connection, DatabaseAction::Schemas, job);
    }
    pub(in crate::app) fn open_direct_schema(
        &mut self,
        connection: Connection,
        schema: String,
        refresh: bool,
    ) {
        self.load_direct_schema(connection, schema, refresh, false);
    }

    pub(in crate::app) fn refresh_open_direct_schema(
        &mut self,
        connection: Connection,
        schema: String,
    ) {
        if self.sql_hosts.get(&connection.id).is_some_and(|host| {
            host.direct
                .as_ref()
                .is_some_and(|tree| tree.scopes.contains_key(&schema))
        }) {
            self.load_direct_schema(connection, schema, true, true);
        }
    }

    fn load_direct_schema(
        &mut self,
        connection: Connection,
        schema: String,
        refresh: bool,
        preserve_tree: bool,
    ) {
        let Some(host) = self.sql_hosts.get_mut(&connection.id) else {
            return;
        };
        let Some(tree) = &mut host.direct else {
            self.select_schema(&connection.id, schema);
            return;
        };
        if !tree.scopes.contains_key(&schema) && tree.scopes.len() >= 32 {
            self.error = Some("已打开32个数据库，请先关闭不使用的数据库".into());
            return;
        }
        if !preserve_tree {
            self.collapsed
                .remove(&super::postgres::schema_key(&connection, &schema));
        }
        if !refresh
            && tree
                .scopes
                .get(&schema)
                .is_some_and(|scope| scope.loading || host.tables.contains_key(&schema))
        {
            return;
        }
        if !preserve_tree && (refresh || !tree.scopes.contains_key(&schema)) {
            self.collapsed
                .insert(backup_tree::Key::new(&connection.id, &schema, "").tree_id());
            self.collapsed
                .insert(query_tree::key(&connection.id, &schema, "", None));
        }
        if refresh && !preserve_tree {
            host.tables.remove(&schema);
        }
        tree.serial = tree.serial.wrapping_add(1);
        tree.scopes.insert(
            schema.clone(),
            Scope {
                loading: true,
                generation: tree.serial,
            },
        );
        let job = Job {
            connection: connection.id.clone(),
            epoch: host.epoch.clone(),
            generation: tree.serial,
            kind: Kind::Tables(schema.clone()),
        };
        let mut routed = connection;
        if matches!(routed.kind(), "MYSQL" | "HIVE") {
            routed = tomato_core::database_scope::select(&routed, &schema);
        }
        self.submit_direct_tree(routed, DatabaseAction::Tables { schema }, job);
    }
    fn submit_direct_tree(&mut self, connection: Connection, action: DatabaseAction, job: Job) {
        match self.queue.submit(Command::Database { connection, action }) {
            Ok(id) => {
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: job.connection.clone(),
                        purpose: Purpose::DirectTree(job),
                    },
                );
            }
            Err(error) => self.direct_tree_reply(job, Err(error)),
        }
    }
    pub(in crate::app) fn direct_tree_reply(&mut self, job: Job, result: anyhow::Result<Output>) {
        let Some(host) = self
            .sql_hosts
            .get_mut(&job.connection)
            .filter(|host| host.epoch == job.epoch)
        else {
            return;
        };
        let Some(tree) = &mut host.direct else {
            return;
        };
        if matches!(job.kind, Kind::Close) {
            if let Err(error) = result {
                self.error = Some(error.to_string());
            }
            return;
        }
        let reset = matches!(&job.kind, Kind::Host(true));
        let result = result.and_then(|output| match output {
            Output::Database(value) => {
                let retained: usize = if reset {
                    0
                } else {
                    host.tables
                        .values()
                        .map(|tables| super::postgres::tables_bytes(tables))
                        .sum()
                };
                let incoming = match &value {
                    DatabaseOutput::Schemas(names) => super::postgres::names_bytes(names),
                    DatabaseOutput::Tables(tables) => super::postgres::tables_bytes(tables),
                    _ => 0,
                };
                anyhow::ensure!(
                    retained.saturating_add(incoming) <= 16 * 1024 * 1024,
                    "数据库树缓存超过16 MiB，请先关闭不使用的数据库"
                );
                Ok(value)
            }
            _ => anyhow::bail!("数据库目录返回类型错误"),
        });
        match job.kind {
            Kind::Host(reset) if tree.generation == job.generation => {
                tree.loading = false;
                match result {
                    Ok(DatabaseOutput::Schemas(names)) => {
                        tree.loaded = true;
                        if reset {
                            tree.scopes.clear();
                            host.tables.clear();
                        } else {
                            tree.scopes.retain(|name, _| names.contains(name));
                            host.tables.retain(|name, _| names.contains(name));
                        }
                        for name in &names {
                            if !tree.scopes.contains_key(name) {
                                self.collapsed
                                    .insert(super::postgres::schema_key(&host.connection, name));
                            }
                        }
                        host.schemas = std::sync::Arc::new(names);
                    }
                    Err(error) => self.error = Some(error.to_string()),
                    _ => self.error = Some("数据库列表返回类型错误".into()),
                }
            }
            Kind::Tables(schema) => {
                let Some(scope) = tree
                    .scopes
                    .get_mut(&schema)
                    .filter(|scope| scope.generation == job.generation)
                else {
                    return;
                };
                scope.loading = false;
                match result {
                    Ok(DatabaseOutput::Tables(tables)) => {
                        if !host.tables.contains_key(&schema) {
                            for folder in [db_tree::Folder::Tables, db_tree::Folder::Views] {
                                self.collapsed.insert(folder.key(&host.connection, &schema));
                            }
                        }
                        host.tables.insert(schema, std::sync::Arc::new(tables));
                    }
                    Err(error) => self.error = Some(error.to_string()),
                    _ => self.error = Some("对象目录返回类型错误".into()),
                }
            }
            Kind::Close => {
                if let Err(error) = result {
                    self.error = Some(error.to_string());
                }
            }
            _ => {}
        }
    }
    pub(in crate::app) fn close_direct_database(
        &mut self,
        connection: Connection,
        database: String,
    ) {
        let same_scope = |tab: &&Tab| matches!(&tab.panel, Panel::Sql { connection: c, workbench, .. } if c.id == connection.id && tabs::workbench::query_database(c, &workbench.schema) == database);
        let structural = |tab: &&Tab| matches!(&tab.panel, Panel::Sql { workbench, .. } if workbench.table.is_some() || workbench.designer.is_some() || workbench.browser.is_some());
        let affected = self
            .tabs
            .iter()
            .filter(same_scope)
            .filter(structural)
            .collect::<Vec<_>>();
        if affected.iter().any(|tab| tab.dirty()) {
            self.error = Some("此数据库还有未保存的内容，请先保存或取消修改后关闭数据库".into());
            return;
        }
        let ids = affected
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        let kept_query = self
            .tabs
            .iter()
            .filter(same_scope)
            .any(|tab| !structural(&tab));
        let Some(host) = self.sql_hosts.get(&connection.id) else {
            return;
        };
        if !kept_query && connection.kind() != "ORACLE" {
            let job = Job {
                connection: connection.id.clone(),
                epoch: host.epoch.clone(),
                generation: 0,
                kind: Kind::Close,
            };
            match self.queue.submit(Command::DisconnectDatabase {
                id: connection.id.clone(),
                database: database.clone(),
            }) {
                Ok(id) => {
                    self.pending.insert(
                        id,
                        Pending {
                            tab_id: connection.id.clone(),
                            purpose: Purpose::DirectTree(job),
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
        if let Some(host) = self.sql_hosts.get_mut(&connection.id) {
            host.tables.remove(&database);
            if !kept_query && connection.kind() != "ORACLE" {
                host.transactions.remove(&database);
            }
            if let Some(tree) = &mut host.direct {
                tree.scopes.remove(&database);
            }
        }
        self.collapsed
            .insert(super::postgres::schema_key(&connection, &database));
    }
}

#[cfg(test)]
mod tests;
