use super::*;
use tomato_core::workbench::TableInfo;
mod backups;
mod postgres;

pub(super) enum Node<'a> {
    Remote(&'a Connection, &'a super::remote_tree::Entry),
    Backup(
        &'a Connection,
        &'a str,
        &'a str,
        Option<&'a tomato_core::backup::Item>,
        Option<&'a str>,
    ),
    Connection(&'a Connection),
    Schema(&'a Connection, &'a str, bool),
    Database(&'a Connection, &'a str, bool),
    Objects(&'a Connection, &'a str, Folder),
    Table(&'a Connection, &'a TableInfo),
    Query(
        &'a Connection,
        &'a str,
        &'a str,
        Option<&'a tomato_core::queries::Entry>,
    ),
    Ssh(
        &'a Connection,
        tomato_services::terminal::ssh::tools::Section,
    ),
    Container(&'a Connection, &'a super::ssh_tree::Container),
    LocalFile(&'a Connection, &'a tomato_services::files::Entry),
}
pub(super) struct Row<'a> {
    pub node: Node<'a>,
    pub depth: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Folder {
    Tables,
    Views,
    Functions,
}
impl Folder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tables => "表",
            Self::Views => "视图",
            Self::Functions => "函数",
        }
    }
    pub fn key(self, connection: &Connection, schema: &str) -> String {
        format!(
            "object-folder:{}:{}",
            connection.id,
            serde_json::to_string(&(connection.text("database"), schema, self.label()))
                .unwrap_or_default()
        )
    }
}
impl Row<'_> {
    pub fn name(&self) -> &str {
        match &self.node {
            Node::Backup(_, _, _, item, _) => item.map_or("备份", |item| item.name.as_str()),
            Node::Remote(_, e) => &e.name,
            Node::Connection(connection) => &connection.name,
            Node::Schema(_, schema, _) => schema,
            Node::Database(_, name, _) => name,
            Node::Objects(_, _, folder) => folder.label(),
            Node::Table(_, table) => &table.table.name,
            Node::Query(_, _, _, entry) => entry.map_or("查询", |entry| entry.name.as_str()),
            Node::Ssh(_, section) => section.label(),
            Node::Container(_, container) => &container.name,
            Node::LocalFile(_, entry) => &entry.name,
        }
    }
    pub fn key(&self) -> String {
        match &self.node {
            Node::Backup(c, database, path, item, view) => {
                let key = super::backup_tree::Key::new(&c.id, database, path);
                item.map_or_else(
                    || key.view_id(*view),
                    |item| {
                        if item.directory {
                            key.child(&item.name).view_id(*view)
                        } else {
                            format!("{}:file:{}", key.view_id(*view), item.name)
                        }
                    },
                )
            }
            Node::Remote(_, e) => e.key.clone(),
            Node::Connection(c) => c.id.clone(),
            Node::Schema(c, s, _) => super::sql_tree::postgres::schema_key(c, s),
            Node::Database(c, name, _) => super::sql_tree::postgres::database_key(&c.id, name),
            Node::Objects(c, s, folder) => folder.key(c, s),
            Node::Table(c, t) if c.kind() == "POSTGRESQL" => format!(
                "pg-table:{}:{}",
                c.id,
                serde_json::to_string(&(c.text("database"), &t.table)).unwrap_or_default()
            ),
            Node::Table(c, t) => format!("table:{}:{}:{}", c.id, t.table.schema, t.table.name),
            Node::Query(c, namespace, path, entry) => super::query_tree::key(
                &c.id,
                namespace,
                path,
                entry.map(|entry| entry.name.as_str()),
            ),
            Node::Ssh(c, s) => format!("ssh:{}:{s:?}", c.id),
            Node::Container(c, row) => format!("container:{}:{}", c.id, row.id),
            Node::LocalFile(c, entry) => super::local_tree::key(&c.id, &entry.path),
        }
    }
    pub fn icon(&self) -> String {
        match &self.node {
            Node::Backup(_, _, _, item, _) => if item.is_some_and(|item| item.directory) {
                "images/connect/folder.png"
            } else {
                "images/connect/backup.png"
            }
            .into(),
            Node::Query(_, _, _, entry) => if entry.is_some_and(|entry| entry.directory) {
                "images/connect/folder.png"
            } else {
                "images/connect/query.png"
            }
            .into(),
            Node::LocalFile(_, entry) => if entry.directory {
                "images/connect/folder.png"
            } else if super::local_tree::markdown(&entry.path) {
                "images/connect/md.png"
            } else {
                "images/connect/code.png"
            }
            .into(),
            Node::Remote(_, e) => e.icon.into(),
            Node::Connection(c) => crate::icons::for_connection(c),
            Node::Database(_, _, opened) => if *opened {
                "images/connect/database.png"
            } else {
                "images/connect/database_gray.png"
            }
            .into(),
            Node::Schema(c, _, expanded) => match (c.kind(), expanded) {
                ("POSTGRESQL", true) => "images/connect/mod_open.png",
                ("POSTGRESQL", false) => "images/connect/mod.png",
                (_, true) => "images/connect/database.png",
                (_, false) => "images/connect/database_gray.png",
            }
            .into(),
            Node::Table(_, table) => if table.view {
                "images/connect/view.png"
            } else {
                "images/connect/table.png"
            }
            .into(),
            Node::Ssh(_, s) => s.icon().into(),
            Node::Objects(_, _, folder) => format!(
                "images/connect/{}",
                match folder {
                    Folder::Tables => "table.png",
                    Folder::Views => "view.png",
                    Folder::Functions => "function.png",
                }
            ),
            Node::Container(_, row) => if row.running {
                "images/connect/docker.png"
            } else {
                "gray:images/connect/docker.png"
            }
            .into(),
        }
    }
}
pub(super) struct Hosts<'a> {
    pub remote: &'a std::collections::HashMap<String, super::remote_tree::Host>,
    pub backups: &'a super::backup_tree::Cache,
    pub queries: &'a super::query_tree::Controller,
    pub sql: &'a std::collections::HashMap<String, super::sql_tree::Host>,
    pub ssh: &'a std::collections::HashMap<String, super::ssh_tree::Host>,
    pub local: &'a std::collections::HashMap<String, super::local_tree::Host>,
    pub mongo: &'a std::collections::HashMap<String, super::mongo_tree::Host>,
    pub redis: &'a std::collections::HashMap<String, super::redis_tree::Host>,
}
pub(super) fn rows<'a>(
    store: &'a Store,
    _tabs: &'a [Tab],
    collapsed: &std::collections::HashSet<String>,
    hosts: Hosts<'a>,
    filter: &str,
) -> Vec<Row<'a>> {
    let Hosts {
        remote: remote_hosts,
        backups: backup_cache,
        queries,
        sql: sql_hosts,
        ssh: ssh_hosts,
        local: local_trees,
        mongo: mongo_hosts,
        redis: redis_hosts,
    } = hosts;
    let mut result = Vec::new();
    let mut hidden = None;
    let tree = tomato_core::tree::ConnectionTree::new(&store.connections);
    for (connection, depth) in tree.rows() {
        if filter.is_empty() {
            if hidden.is_some_and(|hidden| depth > hidden) {
                continue;
            }
            hidden = None;
            if collapsed.contains(&connection.id) {
                hidden = Some(depth);
            }
        }
        let matches = filter.is_empty()
            || connection.name.to_lowercase().contains(filter)
            || remote_hosts.get(&connection.id).is_some_and(|h| {
                h.entries
                    .iter()
                    .any(|e| e.name.to_lowercase().contains(filter))
            })
            || connection.kind().to_lowercase().contains(filter)
            || connection.text("host").to_lowercase().contains(filter);
        let local_matches = !filter.is_empty()
            && local_trees.get(&connection.id).is_some_and(|host| {
                host.directories
                    .values()
                    .flatten()
                    .any(|entry| entry.name.to_lowercase().contains(filter))
            });
        let mongo_matches = !filter.is_empty()
            && mongo_hosts.get(&connection.id).is_some_and(|host| {
                host.databases
                    .iter()
                    .any(|d| d.to_lowercase().contains(filter))
                    || host
                        .collections
                        .values()
                        .flatten()
                        .any(|t| t.table.name.to_lowercase().contains(filter))
            });
        let redis_matches = redis_hosts.get(&connection.id).is_some_and(|h| {
            h.databases
                .iter()
                .any(|d| d.to_lowercase().contains(filter))
        });
        let sql_matches = !filter.is_empty()
            && sql_hosts.get(&connection.id).is_some_and(|host| {
                host.pg.as_ref().is_some_and(|pg| pg.matches(filter))
                    || host
                        .schemas
                        .iter()
                        .any(|schema| schema.to_lowercase().contains(filter))
                    || host.tables.values().any(|tables| {
                        tables
                            .iter()
                            .any(|table| table.table.name.to_lowercase().contains(filter))
                    })
            });
        if matches || local_matches || mongo_matches || redis_matches || sql_matches {
            result.push(Row {
                node: Node::Connection(connection),
                depth,
            });
        }
        if filter.is_empty() && collapsed.contains(&connection.id) {
            continue;
        }
        if let Some(host) = local_trees.get(&connection.id) {
            LocalRows {
                rows: &mut result,
                connection,
                host,
                collapsed,
                filter,
            }
            .visit(&host.root, depth + 1, matches, 0);
        }
        if connection.kind() == "SSH"
            && let Some(host) = ssh_hosts.get(&connection.id)
        {
            use tomato_services::terminal::ssh::tools::Section;
            for section in Section::TREE {
                if matches || section.label().to_lowercase().contains(filter) {
                    result.push(Row {
                        node: Node::Ssh(connection, section),
                        depth: depth + 1,
                    });
                    if section == Section::Docker
                        && !collapsed.contains(&format!("ssh:{}:Docker", connection.id))
                    {
                        for container in &host.containers {
                            if matches || container.name.to_lowercase().contains(filter) {
                                result.push(Row {
                                    node: Node::Container(connection, container),
                                    depth: depth + 2,
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Some(host) = remote_hosts.get(&connection.id) {
            super::remote_tree::append(
                &mut result,
                connection,
                host,
                collapsed,
                if matches { "" } else { filter },
                depth,
            );
        }
        if let Some(host) = redis_hosts.get(&connection.id) {
            for database in &host.databases {
                if matches || database.to_lowercase().contains(filter) {
                    result.push(Row {
                        node: Node::Schema(&host.connection, database, false),
                        depth: depth + 1,
                    });
                }
            }
        }
        if let Some(host) = mongo_hosts.get(&connection.id) {
            for database in &host.databases {
                let expanded = host.collections.contains_key(database)
                    && !collapsed.contains(&format!("schema:{}:{database}", connection.id));
                if matches
                    || database.to_lowercase().contains(filter)
                    || host.collections.get(database).is_some_and(|tables| {
                        tables
                            .iter()
                            .any(|t| t.table.name.to_lowercase().contains(filter))
                    })
                {
                    result.push(Row {
                        node: Node::Schema(
                            &host.connection,
                            database,
                            host.collections.contains_key(database),
                        ),
                        depth: depth + 1,
                    });
                }
                if expanded || !filter.is_empty() {
                    for collection in host.collections.get(database).into_iter().flatten() {
                        if matches || collection.table.name.to_lowercase().contains(filter) {
                            result.push(Row {
                                node: Node::Table(&host.connection, collection),
                                depth: depth + 2,
                            });
                        }
                    }
                }
            }
        }
        if let Some(host) = sql_hosts.get(&connection.id) {
            if host.pg.is_some() {
                postgres::append(
                    &mut result,
                    host,
                    queries,
                    backup_cache,
                    collapsed,
                    filter,
                    depth,
                    matches,
                );
                continue;
            }
            for schema in host.schemas.iter() {
                let opened = host.tables.contains_key(schema)
                    || host
                        .direct
                        .as_ref()
                        .is_some_and(|tree| tree.scopes.contains_key(schema));
                let expanded =
                    opened && !collapsed.contains(&format!("schema:{}:{schema}", connection.id));
                let schema_matches = matches || schema.to_lowercase().contains(filter);
                let table_matches = !filter.is_empty()
                    && host.tables.get(schema).is_some_and(|tables| {
                        tables
                            .iter()
                            .any(|table| table.table.name.to_lowercase().contains(filter))
                    });
                if schema_matches || table_matches {
                    result.push(Row {
                        node: Node::Schema(&host.connection, schema, opened),
                        depth: depth + 1,
                    });
                }
                // Searching may reveal cached descendants, but must not manufacture
                // object folders for databases that have never been opened.
                if opened && (expanded || !filter.is_empty()) && (schema_matches || table_matches) {
                    let folders = matches!(
                        connection.kind(),
                        "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
                    );
                    if folders {
                        for folder in [Folder::Tables, Folder::Views, Folder::Functions] {
                            result.push(Row {
                                node: Node::Objects(&host.connection, schema, folder),
                                depth: depth + 2,
                            });
                            if folder == Folder::Functions
                                || (filter.is_empty()
                                    && collapsed.contains(&folder.key(&host.connection, schema)))
                            {
                                continue;
                            }
                            for table in host
                                .tables
                                .get(schema)
                                .into_iter()
                                .flat_map(|tables| tables.iter())
                                .filter(|table| table.view == (folder == Folder::Views))
                            {
                                if schema_matches
                                    || table.table.name.to_lowercase().contains(filter)
                                {
                                    result.push(Row {
                                        node: Node::Table(&host.connection, table),
                                        depth: depth + 3,
                                    });
                                }
                            }
                        }
                    }
                    for table in host
                        .tables
                        .get(schema)
                        .into_iter()
                        .flat_map(|tables| tables.iter())
                    {
                        if !folders
                            && (schema_matches || table.table.name.to_lowercase().contains(filter))
                        {
                            result.push(Row {
                                node: Node::Table(&host.connection, table),
                                depth: depth + 2,
                            });
                        }
                    }
                    if let Some(tree) = queries.tree(&connection.id, schema) {
                        result.push(Row {
                            node: Node::Query(&host.connection, schema, "", None),
                            depth: depth + 2,
                        });
                        append_queries(
                            &mut result,
                            &host.connection,
                            schema,
                            tree,
                            "",
                            depth + 3,
                            collapsed,
                            filter,
                            0,
                        );
                    }
                    if matches!(host.connection.kind(), "MYSQL" | "ORACLE") {
                        result.push(Row {
                            node: Node::Backup(&host.connection, schema, "", None, None),
                            depth: depth + 2,
                        });
                        backups::append(
                            &mut result,
                            &host.connection,
                            schema,
                            backup_cache,
                            "",
                            depth + 3,
                            collapsed,
                            if schema_matches { "" } else { filter },
                            0,
                            None,
                        );
                    }
                }
            }
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn append_queries<'a>(
    rows: &mut Vec<Row<'a>>,
    connection: &'a Connection,
    namespace: &'a str,
    tree: &'a super::query_tree::Tree,
    path: &'a str,
    depth: usize,
    collapsed: &std::collections::HashSet<String>,
    filter: &str,
    level: usize,
) {
    if level >= 64
        || (filter.is_empty()
            && collapsed.contains(&super::query_tree::key(
                &connection.id,
                namespace,
                path,
                None,
            )))
    {
        return;
    }
    let Some(directory) = tree.directories.get(path) else {
        return;
    };
    for entry in &directory.entries {
        let start = rows.len();
        let matches = filter.is_empty() || entry.name.to_lowercase().contains(filter);
        rows.push(Row {
            node: Node::Query(connection, namespace, path, Some(entry)),
            depth,
        });
        if entry.directory
            && let Ok(child) = directory.scope.child(&entry.name)
            && let Some((child_path, _)) = tree.directories.get_key_value(&child.path)
            && (!collapsed.contains(&super::query_tree::key(
                &connection.id,
                namespace,
                path,
                Some(&entry.name),
            )) || !filter.is_empty())
        {
            append_queries(
                rows,
                connection,
                namespace,
                tree,
                child_path,
                depth + 1,
                collapsed,
                filter,
                level + 1,
            );
        }
        if !matches && rows.len() == start + 1 {
            rows.pop();
        }
    }
}

struct LocalRows<'a, 'b> {
    rows: &'b mut Vec<Row<'a>>,
    connection: &'a Connection,
    host: &'a super::local_tree::Host,
    collapsed: &'b std::collections::HashSet<String>,
    filter: &'b str,
}
impl LocalRows<'_, '_> {
    fn visit(&mut self, path: &std::path::Path, depth: usize, parent_matches: bool, level: usize) {
        if level >= 64 {
            return;
        }
        let host = self.host;
        if let Some(entries) = host.directories.get(path) {
            for entry in entries {
                let matches = parent_matches || entry.name.to_lowercase().contains(self.filter);
                let start = self.rows.len();
                if matches || entry.directory {
                    self.rows.push(Row {
                        node: Node::LocalFile(self.connection, entry),
                        depth,
                    });
                }
                if entry.directory
                    && (!self.filter.is_empty()
                        || !self
                            .collapsed
                            .contains(&super::local_tree::key(&self.connection.id, &entry.path)))
                {
                    self.visit(&entry.path, depth + 1, matches, level + 1);
                }
                if !matches && entry.directory && self.rows.len() == start + 1 {
                    self.rows.pop();
                }
            }
        }
    }
}
impl TomatoApp {
    pub(super) fn select_schema(&mut self, connection_id: &str, schema: String) {
        if self.redis_hosts.contains_key(connection_id) {
            if let Some(number) = schema
                .strip_prefix("db")
                .and_then(|s| s.parse::<u32>().ok())
            {
                self.redis_database(connection_id, number);
            }
            return;
        }
        if self.mongo_hosts.contains_key(connection_id) {
            self.mongo_collections(connection_id, schema, false);
            return;
        }
        if let Some(connection) = self
            .sql_hosts
            .get(connection_id)
            .filter(|h| h.direct.is_some())
            .map(|h| h.connection.clone())
        {
            self.open_direct_schema(connection, schema, false);
            return;
        }
        if !self.tabs.iter().any(|tab| tab.source_id == connection_id)
            && let Some(connection) = self
                .sql_hosts
                .get(connection_id)
                .map(|host| host.connection.clone())
        {
            self.open_connection(connection);
        }
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.source_id==connection_id&&matches!(&tab.panel, Panel::Sql {connection,..} if connection.id == connection_id))
        else {
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
        workbench.schema = schema.clone();
        workbench.tables = Default::default();
        let (id, connection) = (tab.id.clone(), connection.clone());
        self.active = Some(id.clone());
        self.submit(
            &id,
            Command::Database {
                connection,
                action: tomato_core::workbench::DatabaseAction::SelectSchema { schema },
            },
            Purpose::Normal,
        );
    }
    pub(super) fn open_table(&mut self, connection: Connection, table: TableInfo, structure: bool) {
        self.activate_object_tab(connection, table, structure);
        if let Some(id) = self.active.clone() {
            self.load_object_tab(&id);
        }
    }

    pub(super) fn activate_object_tab(
        &mut self,
        connection: Connection,
        table: TableInfo,
        structure: bool,
    ) {
        if connection.kind() == "MONGODB" {
            self.mongo_collection(
                connection,
                tomato_services::mongo::Target {
                    database: table.table.schema,
                    collection: table.table.name,
                },
            );
            return;
        }
        let connection = crate::tabs::workbench::query_connection(&connection, &table.table.schema);
        let source = format!(
            "dbtable:{}:{}:{}",
            if structure { "structure" } else { "data" },
            connection.id,
            serde_json::to_string(&(connection.text("database"), &table.table)).unwrap_or_default()
        );
        if let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        match tabs::create_tab(
            connection.clone(),
            self.runtime.as_ref().map(|r| r.handle()),
        ) {
            Ok(mut tab) => {
                tab.source_id = source;
                tab.title = if !structure && connection.kind() == "SQLITE" {
                    table.table.name.clone()
                } else {
                    format!(
                        "{}@{}({}:{})-{}{}",
                        table.table.name,
                        crate::tabs::workbench::query_database(&connection, &table.table.schema),
                        connection.text("host"),
                        connection.number("port"),
                        if table.view { "视图" } else { "表" },
                        if structure { "结构" } else { "" }
                    )
                };
                if let Panel::Sql { workbench, .. } = &mut tab.panel {
                    workbench.schema = table.table.schema.clone();
                    workbench.table = Some(table.table);
                    workbench.table_is_view = table.view;
                    workbench.structure = structure;
                    workbench.object_load_pending = true;
                    workbench.transaction = self.sql_transaction(&connection, &workbench.schema);
                }
                self.active = Some(tab.id.clone());
                self.tabs.push(tab);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(super) fn load_object_tab(&mut self, id: &str) -> bool {
        use tomato_core::workbench::{DatabaseAction, PageRequest};

        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return false;
        };
        if !std::mem::take(&mut workbench.object_load_pending) {
            return false;
        }
        let Some(table) = workbench.table.clone() else {
            return false;
        };
        let action = if !workbench.structure {
            DatabaseAction::Page {
                request: PageRequest {
                    table,
                    offset: 0,
                    limit: 1000,
                    sort: None,
                    query: Default::default(),
                },
            }
        } else if matches!(
            connection.kind(),
            "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
        ) {
            match crate::tabs::table_design::Draft::new(connection.kind(), table.schema.clone()) {
                Ok(mut draft) => {
                    draft.busy = true;
                    workbench.designer = Some(Box::new(draft));
                }
                Err(error) => {
                    tab.error = Some(error.to_string());
                    return true;
                }
            }
            DatabaseAction::DesignStructure { table }
        } else {
            DatabaseAction::Definition { table }
        };
        let command = Command::Database {
            connection: connection.clone(),
            action,
        };
        self.submit(id, command, Purpose::Normal);
        true
    }
}
