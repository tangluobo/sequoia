//! Submit the existing use cases; no filesystem or database work runs in the view.
use super::*;
impl TomatoApp {
    pub(in crate::app) fn commit_tree_name(&mut self, commit: Commit) {
        let Commit { target, name } = commit;
        if !matches!(target, Target::Connection(_) | Target::Object(..))
            && (matches!(name.as_str(), "." | "..") || name.contains(['/', '\\', '\0', '\r', '\n']))
        {
            self.status = "重命名失败：名称不能包含路径分隔符".into();
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            match target {
                Target::File(connection, path) => {
                    let command = self.tree_mutation(
                        &connection,
                        tomato_services::files::tree::Action::Rename { path, name },
                    );
                    let id = self.queue.submit(command)?;
                    self.pending.insert(
                        id,
                        Pending {
                            tab_id: format!("local-mutation:{}", connection.id),
                            purpose: Purpose::LocalMutation { connection },
                        },
                    );
                }
                Target::Object(connection, table) => {
                    self.rename_tree_object(connection, table, name)?
                }
                Target::Connection(connection) => {
                    let mut next = self.store.clone();
                    let current = next
                        .connections
                        .iter_mut()
                        .find(|c| c.id == connection.id)
                        .ok_or_else(|| anyhow::anyhow!("连接已被删除"))?;
                    anyhow::ensure!(
                        same_connection(current, &connection),
                        "连接已被其他操作修改，请重新选择后改名"
                    );
                    current.name = name;
                    self.persist_store(next);
                }
                Target::Query {
                    connection,
                    namespace,
                    path,
                    entry,
                } => self.rename_query_tree_item(&connection.id, &namespace, &path, entry, name)?,
                Target::Backup(target) => self.rename_backup_tree_item(target, name)?,
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.context.request_repaint(),
            Err(error) => self.status = format!("重命名失败：{error:#}"),
        }
    }
}
