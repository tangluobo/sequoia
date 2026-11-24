//! Bound sidebar metadata separately from SQL editor buffers and database metadata.
use super::*;
impl Controller {
    pub fn touch(&mut self, connection: &str, namespace: &str, path: &str) {
        self.clock = self.clock.wrapping_add(1);
        self.usage.insert(
            (connection.into(), namespace.into(), path.into()),
            self.clock,
        );
    }
    pub fn trim(&mut self, connection: &str, namespace: &str, path: &str) {
        let keep = (connection.to_owned(), namespace.to_owned(), path.to_owned());
        loop {
            let mut bytes = 0usize;
            let mut count = 0usize;
            let mut oldest = None;
            for (connection, trees) in &self.trees {
                for (namespace, tree) in trees {
                    for (path, directory) in &tree.directories {
                        count += 1;
                        bytes = bytes.saturating_add(
                            directory
                                .entries
                                .capacity()
                                .saturating_mul(std::mem::size_of::<Entry>()),
                        );
                        bytes = directory.entries.iter().fold(bytes, |bytes, entry| {
                            bytes.saturating_add(entry.name.capacity())
                        });
                        let key = (connection.clone(), namespace.clone(), path.clone());
                        let age = self.usage.get(&key).copied().unwrap_or(0);
                        if key != keep
                            && !directory.loading
                            && oldest.as_ref().is_none_or(|(_, old)| age < *old)
                        {
                            oldest = Some((key, age));
                        }
                    }
                }
            }
            if count <= 256 && bytes <= 8 * 1024 * 1024 {
                break;
            }
            let Some(((connection, namespace, path), _)) = oldest else {
                break;
            };
            if let Some(tree) = self
                .trees
                .get_mut(&connection)
                .and_then(|trees| trees.get_mut(&namespace))
            {
                tree.directories.remove(&path);
            }
            self.usage.remove(&(connection, namespace, path));
        }
        self.usage.retain(|(connection, namespace, path), _| {
            self.trees
                .get(connection)
                .and_then(|trees| trees.get(namespace))
                .is_some_and(|tree| tree.directories.contains_key(path))
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_budget_evicts_old_directories_and_retains_current_listing() {
        let scope = Scope {
            connection: "test".into(),
            database: "main".into(),
            path: String::new(),
        };
        let mut controller = Controller::default();
        controller
            .trees
            .entry("connection".into())
            .or_default()
            .insert(
                "main".into(),
                Tree {
                    epoch: "current".into(),
                    root: Some(scope.clone()),
                    directories: HashMap::new(),
                    error: None,
                },
            );
        for index in 0..300 {
            let path = index.to_string();
            let directory = Directory {
                scope: Scope {
                    path: path.clone(),
                    ..scope.clone()
                },
                entries: vec![Entry {
                    name: "x".repeat(40 * 1024),
                    directory: false,
                }],
                loading: false,
                error: None,
            };
            controller
                .trees
                .get_mut("connection")
                .unwrap()
                .get_mut("main")
                .unwrap()
                .directories
                .insert(path.clone(), directory);
            controller.touch("connection", "main", &path);
            controller.trim("connection", "main", &path);
        }
        let tree = controller.tree("connection", "main").unwrap();
        assert!(tree.directories.len() < 256);
        assert!(tree.directories.contains_key("299"));
        assert!(!tree.directories.contains_key("0"));
        let bytes: usize = tree
            .directories
            .values()
            .flat_map(|directory| &directory.entries)
            .map(|entry| entry.name.capacity() + std::mem::size_of::<Entry>())
            .sum();
        assert!(bytes <= 8 * 1024 * 1024);
    }
}
