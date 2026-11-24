//! Bounded directory metadata; keys include the actual database, not just a schema.
use std::collections::HashMap;
use tomato_core::backup::{Item, Scope};
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct Key {
    pub connection: String,
    pub database: String,
    pub path: String,
}
impl Key {
    pub fn new(connection: &str, database: &str, path: &str) -> Self {
        Self {
            connection: connection.into(),
            database: database.into(),
            path: path.into(),
        }
    }
    pub fn tree_id(&self) -> String {
        // Length prefixes avoid collisions for names containing ':' or '/'.
        format!(
            "backup:{}:{}{}:{}{}:{}",
            self.connection.len(),
            self.connection,
            self.database.len(),
            self.database,
            self.path.len(),
            self.path
        )
    }
    pub fn view_id(&self, namespace: Option<&str>) -> String {
        namespace.map_or_else(
            || self.tree_id(),
            |namespace| format!("{}:schema:{}:{namespace}", self.tree_id(), namespace.len()),
        )
    }
    pub fn child(&self, name: &str) -> Self {
        Self {
            path: if self.path.is_empty() {
                name.into()
            } else {
                format!("{}/{name}", self.path)
            },
            ..self.clone()
        }
    }
}
pub(crate) struct Directory {
    pub scope: Scope,
    pub generation: u64,
    pub items: Vec<Item>,
    pub loading: bool,
    pub error: Option<String>,
    used: u64,
}
#[derive(Default)]
pub(crate) struct Cache {
    pub directories: HashMap<Key, Directory>,
    clock: u64,
    generation: u64,
}
impl Cache {
    pub fn begin(&mut self, key: Key, scope: Scope) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.directories
            .entry(key.clone())
            .and_modify(|entry| {
                entry.loading = true;
                entry.error = None;
                entry.generation = generation;
                entry.used = self.clock;
            })
            .or_insert(Directory {
                scope,
                generation,
                items: Vec::new(),
                loading: true,
                error: None,
                used: self.clock,
            });
        self.trim(&key);
        generation
    }
    pub fn finish(&mut self, key: &Key, generation: u64, result: anyhow::Result<Vec<Item>>) {
        let Some(directory) = self
            .directories
            .get_mut(key)
            .filter(|entry| entry.generation == generation)
        else {
            return;
        };
        directory.loading = false;
        match result {
            Ok(items) => {
                directory.items = items;
                directory.error = None;
            }
            Err(error) => directory.error = Some(error.to_string()),
        }
        self.trim(key);
    }
    pub fn invalidate_load(&mut self, key: &Key) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(directory) = self.directories.get_mut(key) {
            directory.loading = false;
            directory.generation = self.generation;
        }
    }
    pub fn remove_subtree(&mut self, key: &Key) {
        let prefix = if key.path.is_empty() {
            String::new()
        } else {
            format!("{}/", key.path)
        };
        self.directories.retain(|candidate, _| {
            candidate.connection != key.connection
                || candidate.database != key.database
                || (candidate.path != key.path && !candidate.path.starts_with(&prefix))
        });
    }
    pub fn retain_connections(&mut self, ids: &std::collections::HashSet<&str>) {
        self.directories
            .retain(|key, _| ids.contains(key.connection.as_str()));
    }
    fn bytes(&self) -> usize {
        self.directories
            .iter()
            .fold(0usize, |sum, (key, directory)| {
                directory.items.iter().fold(
                    sum.saturating_add(key.connection.capacity())
                        .saturating_add(key.database.capacity())
                        .saturating_add(key.path.capacity())
                        .saturating_add(directory.scope.connection.capacity())
                        .saturating_add(directory.scope.database.capacity())
                        .saturating_add(directory.scope.path.capacity())
                        .saturating_add(directory.items.capacity() * std::mem::size_of::<Item>())
                        .saturating_add(directory.error.as_ref().map_or(0, String::capacity))
                        .saturating_add(256),
                    |sum, item| sum.saturating_add(item.name.capacity()),
                )
            })
    }
    fn trim(&mut self, keep: &Key) {
        while self.directories.len() > 64 || self.bytes() > 8 * 1024 * 1024 {
            let oldest = self
                .directories
                .iter()
                .filter(|(key, _)| *key != keep)
                .min_by_key(|(_, directory)| directory.used)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.directories.remove(&oldest);
            } else {
                if let Some(directory) = self.directories.get_mut(keep) {
                    directory.items.clear();
                    directory.items.shrink_to_fit();
                    directory.error = Some("备份目录超过 8 MiB".into());
                }
                break;
            }
        }
    }
}
