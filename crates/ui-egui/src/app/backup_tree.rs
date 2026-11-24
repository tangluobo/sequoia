//! Legacy backup tree routing, separate from the archive engine and backup wizard.
use super::*;
use tomato_core::backup::{FileAction, FileOutput, FileRequest, Item, Scope};
mod cache;
mod dialogs;
#[cfg(test)]
mod tests;
pub(super) use cache::{Cache, Directory, Key};
pub(super) use dialogs::Dialog;
pub(super) struct Job {
    key: Key,
    scope: Scope,
    generation: u64,
    kind: JobKind,
}
impl Job {
    pub(super) fn changes_names(&self) -> bool {
        matches!(self.kind, JobKind::Change | JobKind::Rename(_))
    }
}
enum JobKind {
    List,
    Change,
    Rename(Option<String>),
    Directory,
}
#[derive(Clone)]
pub(super) struct Target {
    pub connection: Connection,
    pub database: String,
    pub path: String,
    pub item: Option<Item>,
    pub namespace: Option<String>,
}
pub(super) fn target(
    connection: &Connection,
    database: &str,
    path: &str,
    item: Option<&Item>,
    namespace: Option<&str>,
) -> Target {
    Target {
        connection: connection.clone(),
        database: database.into(),
        path: path.into(),
        item: item.cloned(),
        namespace: namespace.map(str::to_owned),
    }
}
impl Target {
    pub fn directory(&self) -> anyhow::Result<Scope> {
        let scope = Scope {
            connection: self.connection.name.clone(),
            database: self.database.clone(),
            path: self.path.clone(),
        };
        if let Some(item) = &self.item
            && item.directory
        {
            scope.child(&item.name)
        } else {
            Ok(scope)
        }
    }
    pub fn parent(&self) -> Scope {
        Scope {
            connection: self.connection.name.clone(),
            database: self.database.clone(),
            path: self.path.clone(),
        }
    }
}
impl TomatoApp {
    pub(super) fn rename_backup_tree_item(
        &mut self,
        target: Target,
        name: String,
    ) -> anyhow::Result<()> {
        let scope = target.parent();
        let item = target
            .item
            .ok_or_else(|| anyhow::anyhow!("请选择备份文件或目录"))?;
        self.backup_file_command(
            &target.connection.id,
            scope,
            FileAction::Rename { item, name },
            JobKind::Rename(target.namespace),
        )
    }
    pub(super) fn backup_action(&mut self, target: Target, verb: &str) {
        let result = (|| -> anyhow::Result<()> {
            match verb {
                "expand" | "refresh" => {
                    let scope = target.directory()?;
                    self.collapsed.remove(
                        &Key::new(&target.connection.id, &scope.database, &scope.path)
                            .view_id(target.namespace.as_deref()),
                    );
                    self.list_backups(&target.connection.id, scope)?;
                }
                "new" => {
                    let scope = target.directory()?;
                    if let Some(namespace) = target.namespace {
                        self.open_backup_in(
                            target.connection,
                            scope.database,
                            scope.path,
                            Some(namespace),
                        );
                    } else {
                        self.open_backup(target.connection, scope.database, scope.path);
                    }
                }
                "directory" => {
                    let scope = target.directory()?;
                    self.backup_file_command(
                        &target.connection.id,
                        scope,
                        FileAction::Directory,
                        JobKind::Directory,
                    )?;
                }
                "mkdir" => {
                    self.backup_edit = Some(Dialog::directory(target)?);
                }
                "rename" => {
                    self.backup_edit = Some(Dialog::rename(target)?);
                }
                "delete" => {
                    self.backup_edit = Some(Dialog::delete(target)?);
                }
                "restore" => {
                    let item = target
                        .item
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请选择备份文件"))?;
                    anyhow::ensure!(!item.directory, "请选择备份文件");
                    self.open_restore(
                        target.connection.clone(),
                        target.parent(),
                        item.name.clone(),
                    );
                }
                _ => anyhow::bail!("未知备份操作"),
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
    }
    pub(super) fn list_backups(&mut self, connection: &str, scope: Scope) -> anyhow::Result<()> {
        let key = Key::new(connection, &scope.database, &scope.path);
        if self
            .backups
            .directories
            .get(&key)
            .is_some_and(|entry| entry.loading)
        {
            return Ok(());
        }
        anyhow::ensure!(self.pending.values().filter(|pending|matches!(&pending.purpose,Purpose::BackupTree(job) if matches!(job.kind,JobKind::List))).count()<4,"备份目录加载队列已满");
        self.backup_file_command(connection, scope, FileAction::List, JobKind::List)
    }
    fn backup_file_command(
        &mut self,
        connection: &str,
        scope: Scope,
        action: FileAction,
        kind: JobKind,
    ) -> anyhow::Result<()> {
        let root = self
            .repository
            .location()
            .and_then(|path| path.parent())
            .ok_or_else(|| anyhow::anyhow!("当前工作区没有备份保存目录"))?
            .to_path_buf();
        let key = Key::new(connection, &scope.database, &scope.path);
        let id = self.queue.submit(Command::BackupFiles {
            root,
            request: FileRequest {
                scope: scope.clone(),
                action,
            },
        })?;
        let generation = if matches!(kind, JobKind::List) {
            self.backups.begin(key.clone(), scope.clone())
        } else {
            0
        };
        self.pending.insert(
            id,
            Pending {
                tab_id: String::new(),
                purpose: Purpose::BackupTree(Job {
                    key,
                    scope,
                    generation,
                    kind,
                }),
            },
        );
        Ok(())
    }
    pub(super) fn backup_tree_reply(&mut self, job: Job, result: anyhow::Result<Output>) {
        let namespace = match &job.kind {
            JobKind::Rename(namespace) => namespace.clone(),
            _ => None,
        };
        match job.kind {
            JobKind::List => {
                if self
                    .backups
                    .directories
                    .get(&job.key)
                    .is_none_or(|entry| entry.generation != job.generation)
                {
                    return;
                }
                let result = result.and_then(|output| match output {
                    Output::BackupFiles(FileOutput::Listed { scope, items })
                        if scope.database == job.scope.database && scope.path == job.scope.path =>
                    {
                        Ok(items)
                    }
                    _ => anyhow::bail!("未收到备份目录"),
                });
                self.backups.finish(&job.key, job.generation, result);
            }
            JobKind::Change | JobKind::Rename(_) => match result {
                Ok(Output::BackupFiles(FileOutput::Changed {
                    scope,
                    errors,
                    renamed,
                    deleted,
                })) => {
                    if let Some((old, new)) = renamed {
                        if old.directory {
                            let old_key = job.key.child(&old.name);
                            let opened = self.backups.directories.contains_key(&old_key)
                                && !self
                                    .collapsed
                                    .contains(&old_key.view_id(namespace.as_deref()));
                            self.backups.remove_subtree(&old_key);
                            if opened && let Ok(child) = scope.child(&new.name) {
                                let _ = self.list_backups(&job.key.connection, child);
                            }
                            self.selected_connection =
                                Some(job.key.child(&new.name).view_id(namespace.as_deref()));
                        } else {
                            self.selected_connection = Some(format!(
                                "{}:file:{}",
                                job.key.view_id(namespace.as_deref()),
                                new.name
                            ));
                        }
                    }
                    for item in deleted.into_iter().filter(|item| item.directory) {
                        self.backups.remove_subtree(&job.key.child(&item.name));
                    }
                    self.backups.invalidate_load(&job.key);
                    if let Err(error) = self.list_backups(&job.key.connection, scope) {
                        self.error = Some(error.to_string());
                    }
                    if !errors.is_empty() {
                        self.error = Some(errors.join("\n"));
                    }
                }
                Ok(_) => self.error = Some("备份文件任务返回了错误类型".into()),
                Err(error) => self.error = Some(error.to_string()),
            },
            JobKind::Directory => match result {
                Ok(Output::BackupFiles(FileOutput::Directory { path })) => {
                    if let Err(error) = open::that_detached(path) {
                        self.error = Some(format!("打开目录失败：{error}"));
                    }
                }
                Ok(_) => self.error = Some("未收到备份目录".into()),
                Err(error) => self.error = Some(error.to_string()),
            },
        }
    }
}
