//! Legacy query files: sidebar cache, command routing and editor persistence.
use super::*;
use std::path::Path;
use tomato_core::queries::{self, Entry, Request, Scope};
mod cache;
mod dialogs;
mod documents;
pub(in crate::app) mod identity;
#[cfg(test)]
mod retry_tests;
pub(super) use dialogs::Dialog;

#[derive(Default)]
pub(super) struct Controller {
    pub trees: HashMap<String, std::collections::BTreeMap<String, Tree>>,
    pub dialog: Option<Dialog>,
    databases: HashMap<String, (String, String)>,
    resolving: HashMap<String, String>,
    deferred: Vec<Action>,
    usage: HashMap<(String, String, String), u64>,
    clock: u64,
}
pub(super) struct Tree {
    pub epoch: String,
    pub root: Option<Scope>,
    pub directories: HashMap<String, Directory>,
    pub error: Option<String>,
}
pub(super) struct Directory {
    pub scope: Scope,
    pub entries: Vec<Entry>,
    pub loading: bool,
    pub error: Option<String>,
}
pub(super) struct Job {
    pub connection: String,
    pub namespace: String,
    pub epoch: String,
    pub kind: JobKind,
}
pub(super) enum JobKind {
    Resolve,
    List { path: String },
    Read { tab: String, generation: u64 },
    Save { tab: String, generation: u64 },
    Change { path: String },
    OpenFolder,
}
#[derive(Clone)]
pub(super) enum Action {
    Expand {
        connection: String,
        namespace: String,
        path: String,
    },
    Open {
        connection: String,
        namespace: String,
        path: String,
        name: String,
    },
    New {
        connection: String,
        namespace: String,
        path: String,
    },
    NewDirectory {
        connection: String,
        namespace: String,
        path: String,
    },
    Rename {
        connection: String,
        namespace: String,
        path: String,
        entry: Entry,
    },
    Delete {
        connection: String,
        namespace: String,
        path: String,
        entries: Vec<Entry>,
    },
    Refresh {
        connection: String,
        namespace: String,
        path: String,
    },
    OpenFolder {
        connection: String,
        namespace: String,
        path: String,
    },
    Save(String),
}
pub(super) fn key(connection: &str, namespace: &str, path: &str, file: Option<&str>) -> String {
    format!(
        "query:{connection}:{}",
        serde_json::to_string(&(namespace, path, file)).unwrap_or_default()
    )
}
pub(super) fn row_action(
    connection: &Connection,
    namespace: &str,
    path: &str,
    entry: Option<&Entry>,
    verb: &str,
) -> Action {
    let connection = connection.id.clone();
    let namespace = namespace.to_owned();
    let directory = entry
        .filter(|entry| entry.directory)
        .map(|entry| {
            if path.is_empty() {
                entry.name.clone()
            } else {
                format!("{path}/{}", entry.name)
            }
        })
        .unwrap_or_else(|| path.into());
    match verb {
        "open" => Action::Open {
            connection,
            namespace,
            path: path.into(),
            name: entry.unwrap().name.clone(),
        },
        "new" => Action::New {
            connection,
            namespace,
            path: directory,
        },
        "directory" => Action::NewDirectory {
            connection,
            namespace,
            path: directory,
        },
        "rename" => Action::Rename {
            connection,
            namespace,
            path: path.into(),
            entry: entry.unwrap().clone(),
        },
        "delete" => Action::Delete {
            connection,
            namespace,
            path: path.into(),
            entries: vec![entry.unwrap().clone()],
        },
        "refresh" => Action::Refresh {
            connection,
            namespace,
            path: directory,
        },
        "folder" => Action::OpenFolder {
            connection,
            namespace,
            path: directory,
        },
        _ => Action::Expand {
            connection,
            namespace,
            path: directory,
        },
    }
}
impl Controller {
    pub fn has_children(
        &self,
        connection: &str,
        namespace: &str,
        path: &str,
        entry: Option<&Entry>,
    ) -> bool {
        if entry.is_some_and(|entry| !entry.directory) {
            return false;
        }
        let path = entry
            .map(|entry| {
                if path.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{path}/{}", entry.name)
                }
            })
            .unwrap_or_else(|| path.into());
        self.tree(connection, namespace)
            .and_then(|tree| tree.directories.get(&path))
            .is_some_and(|directory| !directory.entries.is_empty())
    }
    pub fn tree(&self, connection: &str, namespace: &str) -> Option<&Tree> {
        self.trees.get(connection)?.get(namespace)
    }
}
impl TomatoApp {
    pub(super) fn rename_query_tree_item(
        &mut self,
        connection: &str,
        namespace: &str,
        path: &str,
        entry: Entry,
        name: String,
    ) -> anyhow::Result<()> {
        let scope = self
            .query_scope(connection, namespace, path)
            .ok_or_else(|| anyhow::anyhow!("查询目录已关闭或正在重新加载"))?;
        self.query_command(
            connection,
            namespace,
            Request::Rename { scope, entry, name },
            JobKind::Change { path: path.into() },
        )
    }
    fn query_action_connection<'a>(&'a self, action: &'a Action) -> Option<&'a str> {
        match action {
            Action::Save(id) => {
                self.tabs
                    .iter()
                    .find(|tab| &tab.id == id)
                    .and_then(|tab| match &tab.panel {
                        Panel::Sql { connection, .. } => Some(connection.id.as_str()),
                        _ => None,
                    })
            }
            Action::Expand { connection, .. }
            | Action::Open { connection, .. }
            | Action::New { connection, .. }
            | Action::NewDirectory { connection, .. }
            | Action::Rename { connection, .. }
            | Action::Delete { connection, .. }
            | Action::Refresh { connection, .. }
            | Action::OpenFolder { connection, .. } => Some(connection),
        }
    }
    fn query_root(&self) -> anyhow::Result<PathBuf> {
        self.repository
            .location()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("当前工作区没有查询文件保存目录"))
    }
    fn submit_query(&mut self, command: Command, job: Job) -> anyhow::Result<()> {
        let id = self.queue.submit(command)?;
        self.pending.insert(
            id,
            Pending {
                tab_id: String::new(),
                purpose: Purpose::NamedQuery(job),
            },
        );
        Ok(())
    }
    fn query_command(
        &mut self,
        connection: &str,
        namespace: &str,
        request: Request,
        kind: JobKind,
    ) -> anyhow::Result<()> {
        let epoch = self
            .sql_hosts
            .get(connection)
            .ok_or_else(|| anyhow::anyhow!("数据库连接已关闭"))?
            .epoch
            .clone();
        let root = self.query_root()?;
        self.submit_query(
            Command::NamedQueries { root, request },
            Job {
                connection: connection.into(),
                namespace: namespace.into(),
                epoch,
                kind,
            },
        )
    }
    pub(super) fn ensure_query_trees(&mut self) {
        self.queries.databases.retain(|id, (epoch, _)| {
            self.sql_hosts
                .get(id)
                .is_some_and(|host| &host.epoch == epoch)
        });
        self.queries.resolving.retain(|id, epoch| {
            self.sql_hosts
                .get(id)
                .is_some_and(|host| &host.epoch == epoch)
        });
        self.queries.deferred.retain(|action| match action {
            Action::Save(id) => self.tabs.iter().any(|tab| &tab.id == id),
            Action::Expand { connection, .. }
            | Action::Open { connection, .. }
            | Action::New { connection, .. }
            | Action::NewDirectory { connection, .. }
            | Action::Rename { connection, .. }
            | Action::Delete { connection, .. }
            | Action::Refresh { connection, .. }
            | Action::OpenFolder { connection, .. } => self.sql_hosts.contains_key(connection),
        });
        self.queries.trees.retain(|id, trees| {
            if let Some(host) = self.sql_hosts.get(id) {
                trees.retain(|_, tree| tree.epoch == host.epoch);
                !trees.is_empty()
            } else {
                false
            }
        });
        let mut scopes = Vec::new();
        for (id, host) in &self.sql_hosts {
            if self.collapsed.contains(id) {
                continue;
            }
            if let Some(pg) = &host.pg {
                for (name, db) in &pg.databases {
                    if self
                        .collapsed
                        .contains(&super::sql_tree::postgres::database_key(id, name))
                    {
                        continue;
                    }
                    for (namespace, scope) in &db.scopes {
                        if !self
                            .collapsed
                            .contains(&super::sql_tree::postgres::schema_key(
                                &db.connection,
                                namespace,
                            ))
                        {
                            scopes.push((id.clone(), scope.query_key.clone()));
                        }
                    }
                }
                continue;
            }
            for namespace in host.tables.keys() {
                if !self
                    .collapsed
                    .contains(&super::sql_tree::postgres::schema_key(
                        &host.connection,
                        namespace,
                    ))
                {
                    scopes.push((id.clone(), namespace.clone()));
                }
            }
        }
        for (connection, namespace) in scopes {
            if self.queries.tree(&connection, &namespace).is_none()
                && let Err(error) = self.ensure_query_scope(&connection, &namespace)
            {
                self.error = Some(error.to_string());
            }
        }
    }
    fn ensure_query_scope(&mut self, connection: &str, namespace: &str) -> anyhow::Result<()> {
        let host = self
            .sql_hosts
            .get(connection)
            .ok_or_else(|| anyhow::anyhow!("数据库连接已关闭"))?;
        let epoch = host.epoch.clone();
        let database = if let Some(scope) = identity::decode(namespace)? {
            Some(scope.database.to_owned())
        } else if host.connection.kind() == "POSTGRESQL" {
            self.queries
                .databases
                .get(connection)
                .filter(|(source, _)| source == &epoch)
                .map(|(_, database)| database.clone())
        } else {
            Some(namespace.into())
        };
        let scope = database.map(|database| Scope {
            connection: host.connection.name.clone(),
            database,
            path: String::new(),
        });
        self.collapsed.insert(key(connection, namespace, "", None));
        self.queries
            .trees
            .entry(connection.into())
            .or_default()
            .insert(
                namespace.into(),
                Tree {
                    epoch: epoch.clone(),
                    root: scope.clone(),
                    directories: HashMap::new(),
                    error: None,
                },
            );
        if let Some(scope) = scope {
            return self.load_query_directory(connection, namespace, scope);
        }
        if self.queries.resolving.get(connection) != Some(&epoch) {
            let command = Command::Sql {
                connection: host.connection.clone(),
                sql: "SELECT current_database()::text".into(),
            };
            self.submit_query(
                command,
                Job {
                    connection: connection.into(),
                    namespace: namespace.into(),
                    epoch: epoch.clone(),
                    kind: JobKind::Resolve,
                },
            )?;
            self.queries.resolving.insert(connection.into(), epoch);
        }
        Ok(())
    }
    fn query_scope(&self, connection: &str, namespace: &str, path: &str) -> Option<Scope> {
        let root = self.queries.tree(connection, namespace)?.root.as_ref()?;
        Some(Scope {
            path: path.into(),
            ..root.clone()
        })
    }
    fn load_query_directory(
        &mut self,
        connection: &str,
        namespace: &str,
        scope: Scope,
    ) -> anyhow::Result<()> {
        let path = scope.path.clone();
        self.queries.touch(connection, namespace, &path);
        self.query_command(
            connection,
            namespace,
            Request::List(scope.clone()),
            JobKind::List { path: path.clone() },
        )?;
        if let Some(tree) = self
            .queries
            .trees
            .get_mut(connection)
            .and_then(|trees| trees.get_mut(namespace))
        {
            let directory = tree.directories.entry(path).or_insert_with(|| Directory {
                scope,
                entries: Vec::new(),
                loading: false,
                error: None,
            });
            directory.loading = true;
            directory.error = None;
        }
        Ok(())
    }
    pub(super) fn query_action(&mut self, action: Action) {
        let saving = if let Action::Save(id) = &action {
            Some(id.clone())
        } else {
            None
        };
        if let Err(error) = self.query_action_result(action) {
            if let Some(id) = saving
                && let Some(Tab {
                    panel: Panel::Sql { workbench, .. },
                    ..
                }) = self.tabs.iter_mut().find(|tab| tab.id == id)
                && let Some(document) = &mut workbench.document
            {
                document.close_after_save = false;
            }
            self.error = Some(format!("查询操作失败：{error:#}"));
        }
    }
    fn query_action_result(&mut self, action: Action) -> anyhow::Result<()> {
        let (connection, namespace, path) = match &action {
            Action::Save(tab) => {
                return self.request_query_save(tab);
            }
            Action::Expand {
                connection,
                namespace,
                path,
            }
            | Action::Open {
                connection,
                namespace,
                path,
                ..
            }
            | Action::New {
                connection,
                namespace,
                path,
            }
            | Action::NewDirectory {
                connection,
                namespace,
                path,
            }
            | Action::Rename {
                connection,
                namespace,
                path,
                ..
            }
            | Action::Delete {
                connection,
                namespace,
                path,
                ..
            }
            | Action::Refresh {
                connection,
                namespace,
                path,
            }
            | Action::OpenFolder {
                connection,
                namespace,
                path,
            } => (connection.clone(), namespace.clone(), path.clone()),
        };
        if self
            .queries
            .tree(&connection, &namespace)
            .is_none_or(|tree| tree.root.is_none())
        {
            self.ensure_query_scope(&connection, &namespace)?;
        }
        let Some(scope) = self.query_scope(&connection, &namespace, &path) else {
            anyhow::ensure!(
                self.queries.deferred.len() < 64,
                "查询初始化操作已排队，请等待数据库连接完成"
            );
            self.queries.deferred.push(action);
            return Ok(());
        };
        match action {
            Action::Expand { .. } | Action::Refresh { .. } => {
                self.load_query_directory(&connection, &namespace, scope)?
            }
            Action::Open { name, .. } => {
                self.open_query_file(&connection, &namespace, scope, Some(name))?
            }
            Action::New { .. } => self.open_query_file(&connection, &namespace, scope, None)?,
            Action::NewDirectory { .. } => {
                self.queries.dialog = Some(Dialog::directory(connection, namespace, scope))
            }
            Action::Rename { entry, .. } => {
                self.queries.dialog = Some(Dialog::rename(connection, namespace, scope, entry))
            }
            Action::Delete { entries, .. } => {
                self.queries.dialog = Some(Dialog::delete(connection, namespace, scope, entries))
            }
            Action::OpenFolder { .. } => self.query_command(
                &connection,
                &namespace,
                Request::EnsureDirectory(scope),
                JobKind::OpenFolder,
            )?,
            Action::Save(_) => unreachable!(),
        }
        Ok(())
    }
    pub(super) fn query_reply(&mut self, job: Job, result: anyhow::Result<Output>) {
        if matches!(job.kind, JobKind::Read { .. } | JobKind::Save { .. }) {
            self.query_document_reply(job, result);
            return;
        }
        if !self
            .sql_hosts
            .get(&job.connection)
            .is_some_and(|host| host.epoch == job.epoch)
        {
            return;
        }
        match job.kind {
            JobKind::Resolve => {
                self.queries.resolving.remove(&job.connection);
                let database = result.and_then(|output| {
                    let Output::Queries(rows) = output else {
                        anyhow::bail!("未返回数据库名称")
                    };
                    rows.into_iter()
                        .next()
                        .and_then(|row| row.rows.into_iter().next())
                        .and_then(|row| row.into_iter().next())
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("数据库名称为空"))
                });
                match database {
                    Ok(database) => {
                        self.queries
                            .databases
                            .insert(job.connection.clone(), (job.epoch, database.clone()));
                        let name = self.sql_hosts[&job.connection].connection.name.clone();
                        let namespaces = self
                            .queries
                            .trees
                            .get_mut(&job.connection)
                            .into_iter()
                            .flat_map(|trees| trees.iter_mut())
                            .map(|(namespace, tree)| {
                                let scope = Scope {
                                    connection: name.clone(),
                                    database: database.clone(),
                                    path: String::new(),
                                };
                                tree.root = Some(scope.clone());
                                tree.error = None;
                                (namespace.clone(), scope)
                            })
                            .collect::<Vec<_>>();
                        for (namespace, scope) in namespaces {
                            if let Err(error) =
                                self.load_query_directory(&job.connection, &namespace, scope)
                            {
                                self.error = Some(error.to_string());
                            }
                        }
                        let deferred = std::mem::take(&mut self.queries.deferred);
                        for action in deferred {
                            if self.query_action_connection(&action)
                                == Some(job.connection.as_str())
                            {
                                self.query_action(action);
                            } else {
                                self.queries.deferred.push(action);
                            }
                        }
                    }
                    Err(error) => {
                        // Failed initialization must not leave deferred close/save
                        // actions armed, or replay them on an unrelated connection.
                        let actions = std::mem::take(&mut self.queries.deferred);
                        for action in actions {
                            let belongs = self.query_action_connection(&action)
                                == Some(job.connection.as_str());
                            if !belongs {
                                self.queries.deferred.push(action);
                            } else if let Action::Save(id) = action
                                && let Some(Tab {
                                    panel: Panel::Sql { workbench, .. },
                                    ..
                                }) = self.tabs.iter_mut().find(|tab| tab.id == id)
                                && let Some(document) = &mut workbench.document
                            {
                                document.close_after_save = false;
                                document.resave = false;
                            }
                        }
                        if let Some(trees) = self.queries.trees.get_mut(&job.connection) {
                            for tree in trees.values_mut() {
                                tree.error = Some(error.to_string());
                            }
                        }
                        self.error = Some(format!("加载查询失败：{error:#}"));
                    }
                }
            }
            JobKind::List { path } => {
                if let Some(directory) = self
                    .queries
                    .trees
                    .get_mut(&job.connection)
                    .and_then(|trees| trees.get_mut(&job.namespace))
                    .and_then(|tree| tree.directories.get_mut(&path))
                {
                    directory.loading = false;
                    match result {
                        Ok(Output::NamedQueries(queries::Output::Listed { entries, .. })) => {
                            for entry in entries.iter().filter(|entry| {
                                entry.directory
                                    && !directory
                                        .entries
                                        .iter()
                                        .any(|old| old.directory && old.name == entry.name)
                            }) {
                                self.collapsed.insert(key(
                                    &job.connection,
                                    &job.namespace,
                                    &path,
                                    Some(&entry.name),
                                ));
                            }
                            directory.entries = entries
                        }
                        Err(error) => {
                            directory.error = Some(error.to_string());
                            self.error = Some(format!("加载查询目录失败：{error:#}"));
                        }
                        _ => {
                            directory.error = Some("未收到查询列表".into());
                            self.error = Some("加载查询目录失败：未收到查询列表".into());
                        }
                    }
                }
                self.queries.trim(&job.connection, &job.namespace, &path);
            }
            JobKind::Change { path } => {
                self.query_change_reply(&job.connection, &job.namespace, &path, result)
            }
            JobKind::OpenFolder => match result {
                Ok(Output::NamedQueries(queries::Output::Directory { path, .. })) => {
                    if let Err(error) = open::that_detached(path) {
                        self.error = Some(format!("打开目录失败：{error}"));
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
                _ => {}
            },
            _ => unreachable!(),
        }
    }
}
