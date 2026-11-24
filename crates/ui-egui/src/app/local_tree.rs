//! Lazy directory tree controller. Filesystem work is routed through CommandHandler.
use super::*;
use std::collections::HashSet;
mod edit;
mod rename;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Directory,
    Markdown,
    Rename,
    Delete,
}
pub(super) struct Edit {
    pub connection: Connection,
    pub path: PathBuf,
    pub kind: Kind,
    pub name: String,
    pub focus: bool,
    pub busy: bool,
    pub deleting: Vec<PathBuf>,
}
#[derive(Default)]
pub(super) struct Host {
    pub root: PathBuf,
    pub connection: Option<std::sync::Arc<Connection>>,
    pub directories: HashMap<PathBuf, Vec<tomato_services::files::Entry>>,
    pending: HashSet<PathBuf>,
    generation: u64,
}
impl Host {
    pub(super) fn loading(&self, path: &std::path::Path) -> bool {
        self.pending.contains(path)
    }
}
pub(super) fn key(connection: &str, path: &std::path::Path) -> String {
    format!("local:{connection}:{}", path.display())
}
pub(super) fn markdown(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            ["md", "markdown", "mdown", "mkd"]
                .iter()
                .any(|ext| value.eq_ignore_ascii_case(ext))
        })
}
pub(super) fn root(connection: &Connection) -> PathBuf {
    if tomato_services::files::object_tree::is_object(connection) {
        PathBuf::from("/")
    } else {
        PathBuf::from(connection.text("localDirectoryPath"))
    }
}
fn same_source(a: &Connection, b: &Connection) -> bool {
    a.options
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "password" | "savePassword"))
        .all(|(key, value)| b.options.get(key) == Some(value))
        && b.options
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "password" | "savePassword"))
            .all(|(key, value)| a.options.get(key) == Some(value))
        && (b.text("password").is_empty() || a.text("password") == b.text("password"))
}
impl TomatoApp {
    pub(super) fn tree_connection(&self, connection: &Connection) -> Connection {
        self.tree_connection_shared(connection).as_ref().clone()
    }
    pub(super) fn tree_connection_shared(
        &self,
        connection: &Connection,
    ) -> std::sync::Arc<Connection> {
        self.local_trees
            .get(&connection.id)
            .and_then(|host| host.connection.as_ref())
            .filter(|cached| same_source(cached, connection))
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(connection.clone()))
    }
    pub(super) fn tree_mutation(
        &self,
        connection: &Connection,
        action: tomato_services::files::tree::Action,
    ) -> Command {
        if tomato_services::files::object_tree::is_object(connection) {
            Command::ObjectTree {
                connection: self.tree_connection(connection),
                action: tomato_services::files::object_tree::Request::Mutate(action),
            }
        } else {
            Command::LocalTree {
                root: root(connection),
                action,
            }
        }
    }
    pub(super) fn prune_local_trees(&mut self) {
        let mut obsolete = Vec::new();
        self.local_trees.retain(|id, host| {
            let keep = self.store.connections.iter().any(|connection| {
                connection.id == *id
                    && connection.kind() == "LOCAL_DIRECTORY"
                    && root(connection) == host.root
                    && host
                        .connection
                        .as_ref()
                        .is_none_or(|old| same_source(old, connection))
            });
            if !keep {
                obsolete.push(format!("local:{id}:"));
            }
            keep
        });
        for prefix in obsolete {
            self.collapsed.retain(|key| !key.starts_with(&prefix));
        }
    }
    pub(super) fn local_tree_open(
        &mut self,
        connection: &Connection,
        path: Option<PathBuf>,
        refresh: bool,
    ) {
        let object = tomato_services::files::object_tree::is_object(connection);
        let connection = self.tree_connection(connection);
        if object && connection.text("password").is_empty() {
            self.credential = Some(credentials::Prompt::new(connection));
            return;
        }
        let root = root(&connection);
        if !object && !root.is_absolute() {
            self.error =
                Some("目录路径未配置或不是绝对路径，请在连接配置中设置本地目录路径".into());
            return;
        }
        let path = path.unwrap_or_else(|| root.clone());
        if !path.starts_with(&root) {
            self.error = Some("目录不属于此连接".into());
            return;
        }
        let node_key = if path == root {
            connection.id.clone()
        } else {
            key(&connection.id, &path)
        };
        let replacing_root = self
            .local_trees
            .get(&connection.id)
            .is_some_and(|host| host.root != root);
        let clear_all = path == root || replacing_root;
        if refresh || replacing_root {
            let prefix = if clear_all {
                format!("local:{}:", connection.id)
            } else {
                key(&connection.id, &path)
            };
            self.collapsed.retain(|old| {
                if clear_all {
                    !old.starts_with(&prefix)
                } else {
                    !old.strip_prefix(&prefix)
                        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(['/', '\\']))
                }
            });
        }
        let host = self.local_trees.entry(connection.id.clone()).or_default();
        if object
            && host
                .connection
                .as_ref()
                .is_none_or(|old| !same_source(old, &connection))
        {
            host.connection = Some(std::sync::Arc::new(connection.clone()));
        }
        if host.root != root {
            host.root = root;
            host.generation = host.generation.wrapping_add(1);
            host.directories.clear();
            host.pending.clear();
        } else if refresh {
            host.generation = host.generation.wrapping_add(1);
            host.directories
                .retain(|child, _| !child.starts_with(&path));
            host.pending.clear();
        }
        if !refresh && host.directories.contains_key(&path) {
            if !self.collapsed.remove(&node_key) {
                self.collapsed.insert(node_key);
            }
            return;
        }
        if host.pending.contains(&path) {
            return;
        }
        if host.directories.len() + host.pending.len() >= 64 {
            self.error = Some("连接树最多缓存 64 个目录，请刷新连接树释放缓存".into());
            return;
        }
        match self.queue.submit(if object {
            Command::ObjectTree {
                connection: connection.clone(),
                action: tomato_services::files::object_tree::Request::List(path.clone()),
            }
        } else {
            Command::ListVisibleFiles { path: path.clone() }
        }) {
            Ok(id) => {
                self.collapsed.remove(&node_key);
                host.pending.insert(path.clone());
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: format!("local-tree:{}:{}", connection.id, path.display()),
                        purpose: Purpose::LocalTree {
                            connection: connection.id.clone(),
                            path,
                            generation: host.generation,
                        },
                    },
                );
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(super) fn local_tree_reply(
        &mut self,
        id: &str,
        path: &std::path::Path,
        generation: u64,
        result: anyhow::Result<Output>,
    ) {
        let current: usize = self
            .local_trees
            .values()
            .flat_map(|host| host.directories.values())
            .flatten()
            .map(tomato_services::files::Entry::retained_bytes)
            .sum();
        let Some(host) = self.local_trees.get_mut(id) else {
            return;
        };
        if host.generation != generation {
            return;
        }
        host.pending.remove(path);
        match result {
            Ok(Output::Files(entries)) => {
                let previous: usize = host
                    .directories
                    .get(path)
                    .into_iter()
                    .flatten()
                    .map(tomato_services::files::Entry::retained_bytes)
                    .sum();
                let incoming: usize = entries
                    .iter()
                    .map(tomato_services::files::Entry::retained_bytes)
                    .sum();
                if current.saturating_sub(previous).saturating_add(incoming) > 64 * 1024 * 1024 {
                    self.error = Some("连接树列表数据预算超过 64 MiB，请刷新不再浏览的连接".into());
                    return;
                }
                let retained: usize = host
                    .directories
                    .iter()
                    .filter(|(old, _)| old.as_path() != path)
                    .map(|(_, entries)| entries.len())
                    .sum();
                if retained.saturating_add(entries.len()) > 10_000 {
                    self.error = Some("连接树缓存超过 10,000 项，请刷新后展开所需目录".into());
                    return;
                }
                for entry in &entries {
                    if entry.directory && !host.directories.contains_key(&entry.path) {
                        self.collapsed.insert(key(id, &entry.path));
                    }
                }
                host.directories.insert(path.to_owned(), entries);
            }
            Err(error) => self.error = Some(format!("加载目录失败：{error:#}")),
            _ => self.error = Some("目录加载返回了不匹配的结果".into()),
        }
    }
}
