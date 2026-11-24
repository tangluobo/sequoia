//! Move cached paths after a successful rename; never resurrect stale list replies.
use super::*;
fn relocated(path: &std::path::Path, old: &std::path::Path, new: &std::path::Path) -> PathBuf {
    path.strip_prefix(old).map_or_else(
        |_| path.to_owned(),
        |suffix| {
            if suffix.as_os_str().is_empty() {
                new.to_owned()
            } else {
                new.join(suffix)
            }
        },
    )
}
fn remap_key(
    value: String,
    prefix: &str,
    id: &str,
    old: &std::path::Path,
    new: &std::path::Path,
) -> String {
    value
        .strip_prefix(prefix)
        .filter(|p| std::path::Path::new(p).starts_with(old))
        .map(|p| key(id, &relocated(std::path::Path::new(p), old, new)))
        .unwrap_or(value)
}
impl TomatoApp {
    pub(in crate::app) fn local_tree_renamed(
        &mut self,
        connection: &Connection,
        old: &std::path::Path,
        new: &std::path::Path,
    ) {
        let Some(host) = self.local_trees.get_mut(&connection.id) else {
            return;
        };
        host.generation = host.generation.wrapping_add(1);
        let loading = host
            .pending
            .drain()
            .map(|p| relocated(&p, old, new))
            .collect::<Vec<_>>();
        host.directories = std::mem::take(&mut host.directories)
            .into_iter()
            .map(|(path, mut entries)| {
                for e in &mut entries {
                    if e.path.starts_with(old) {
                        let exact = e.path == old;
                        e.path = relocated(&e.path, old, new);
                        if exact {
                            e.name = new
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                        }
                    }
                }
                (relocated(&path, old, new), entries)
            })
            .collect();
        self.pending.retain(
            |_, p| !matches!(&p.purpose,Purpose::LocalTree{connection:id,..}if id==&connection.id),
        );
        let prefix = format!("local:{}:", connection.id);
        self.collapsed = std::mem::take(&mut self.collapsed)
            .into_iter()
            .map(|v| remap_key(v, &prefix, &connection.id, old, new))
            .collect();
        if let Some(v) = self.selected_connection.take() {
            self.selected_connection = Some(remap_key(v, &prefix, &connection.id, old, new));
        }
        if let Some(v) = self.local_anchor.take() {
            self.local_anchor = Some(remap_key(v, &prefix, &connection.id, old, new));
        }
        self.local_selection = std::mem::take(&mut self.local_selection)
            .into_iter()
            .map(|(id, p)| {
                let p = if id == connection.id {
                    relocated(&p, old, new)
                } else {
                    p
                };
                (id, p)
            })
            .collect();
        for path in loading {
            if self
                .local_trees
                .get(&connection.id)
                .is_some_and(|h| !h.directories.contains_key(&path))
            {
                self.local_tree_open(connection, Some(path), false);
            }
        }
    }
}
