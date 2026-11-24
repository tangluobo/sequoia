//! Backup wizard controller. Database actors own I/O; this module owns only drafts.
use super::*;
mod alerts;
mod checkbox;
mod draft;
mod log_view;
#[cfg(test)]
mod native_tests;
mod preview;
mod refresh;
mod restore_source;
mod restore_view;
mod selection;
#[cfg(test)]
mod server_test;
mod template;
#[cfg(test)]
mod tests;
mod tree;
mod tree_input;
mod view;
pub(super) use draft::Draft;
#[derive(Clone, Copy)]
pub(super) enum Job {
    Objects,
    Create,
}
const TEMPLATE: &str = "rustBackupTemplate";

impl TomatoApp {
    pub(super) fn open_restore(
        &mut self,
        connection: Connection,
        scope: tomato_core::backup::Scope,
        name: String,
    ) {
        if let Some(old) = self.backup_dialog.take() {
            old.release(&self.context);
        }
        let mut draft = Draft::new(connection, scope, &self.context);
        draft.loading = false;
        draft.restore = Some(name);
        self.backup_dialog = Some(draft);
    }
    pub(super) fn choose_backup_to_restore(
        &mut self,
        connection: Connection,
        scope: tomato_core::backup::Scope,
    ) {
        if let Some(old) = self.backup_dialog.take() {
            old.release(&self.context);
        }
        let mut draft = Draft::new(connection, scope, &self.context);
        draft.loading = false;
        draft.choose_restore_source = true;
        self.load_restore_source(&mut draft);
        self.backup_dialog = Some(draft);
    }
    fn load_restore_source(&mut self, draft: &mut Draft) {
        if let Err(error) = self.list_backups(&draft.connection.id, draft.scope.clone()) {
            draft.notice = Some(error.to_string());
        }
    }
    pub(super) fn open_backup(&mut self, connection: Connection, database: String, path: String) {
        self.open_backup_in(connection, database, path, None);
    }
    pub(super) fn open_backup_in(
        &mut self,
        connection: Connection,
        database: String,
        path: String,
        namespace: Option<String>,
    ) {
        if let Some(old) = self.backup_dialog.take() {
            old.release(&self.context);
        }
        let scope = tomato_core::backup::Scope {
            connection: connection.name.clone(),
            database: database.clone(),
            path,
        };
        let mut draft = Draft::new(connection.clone(), scope, &self.context);
        draft.namespace = namespace.clone();
        let command = Command::BackupObjects {
            connection,
            namespace: database,
            schema: namespace,
        };
        match self.queue.submit(command) {
            Ok(id) => {
                self.pending.insert(
                    id,
                    Pending {
                        tab_id: draft.id.clone(),
                        purpose: Purpose::Backup(draft.id.clone(), Job::Objects),
                    },
                );
            }
            Err(error) => {
                draft.loading = false;
                draft.load_failed = true;
                draft.tab = 3;
                draft.error = Some(error.to_string());
            }
        }
        self.backup_dialog = Some(draft);
    }
    pub(super) fn backup_reply(&mut self, id: &str, job: Job, result: anyhow::Result<Output>) {
        let Some(draft) = self.backup_dialog.as_mut().filter(|draft| draft.id == id) else {
            return;
        };
        match job {
            Job::Objects => {
                draft.loading = false;
                match result {
                    Ok(Output::BackupObjects(mut objects)) => {
                        if draft.connection.kind() == "POSTGRESQL"
                            && let Some(namespace) = &draft.namespace
                        {
                            objects.retain(|object| object.table.schema == *namespace);
                        }
                        draft.groups = selection::groups(objects);
                        template::apply_groups(draft);
                    }
                    Ok(_) => {
                        draft.load_failed = true;
                        draft.tab = 3;
                        draft.error = Some("备份对象任务返回了错误类型".into());
                    }
                    Err(error) => {
                        draft.load_failed = true;
                        draft.tab = 3;
                        draft.error = Some(format!("加载数据库对象失败：{error:#}"));
                    }
                }
            }
            Job::Create => {
                draft.running = false;
                match result {
                    Ok(Output::BackupDatabase(report)) => {
                        draft.elapsed_ms = Some(report.elapsed_ms);
                        draft.complete = Some(if let Some(name) = &draft.restore {
                            format!(
                                "备份 \"{name}\" 已成功还原到数据库 \"{}\"",
                                draft.scope.database
                            )
                        } else {
                            format!("备份文件已保存:\n{}", report.path.display())
                        });
                    }
                    Ok(_) => {
                        draft.error = Some("备份任务返回了错误类型".into());
                    }
                    Err(error) => {
                        draft.error = Some(format!("错误: {error:#}"));
                        draft.notice = draft.error.clone();
                        draft.tab = if draft.restore.is_some() { 1 } else { 3 };
                    }
                }
            }
        }
        let restored = (matches!(job, Job::Create)
            && draft.restore.is_some()
            && draft.complete.is_some())
        .then(|| tomato_core::database_scope::select(&draft.connection, &draft.scope.database));
        if matches!(job, Job::Create) && draft.restore.is_none() && draft.complete.is_some() {
            let connection = draft.connection.id.clone();
            let scope = draft.scope.clone();
            let key = super::backup_tree::Key::new(&connection, &scope.database, &scope.path);
            self.backups.invalidate_load(&key);
            if let Err(error) = self.list_backups(&connection, scope) {
                self.error = Some(error.to_string());
            }
        }
        if let Some(connection) = restored {
            self.refresh_restored_database(&connection);
        }
    }
    pub(super) fn backup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.backup_dialog.take() else {
            return;
        };
        let mut action = None;
        let viewport = egui::ViewportId::from_hash_of(("backup-dialog", &draft.id));
        let restoring = draft.restore.is_some() || draft.choose_restore_source;
        let directory = draft
            .choose_restore_source
            .then(|| {
                let key = super::backup_tree::Key::new(
                    &draft.connection.id,
                    &draft.scope.database,
                    &draft.scope.path,
                );
                self.backups.directories.get(&key)
            })
            .flatten();
        let size = if restoring {
            [620., 480.]
        } else {
            [720., 580.]
        };
        let mut builder = egui::ViewportBuilder::default()
            .with_title(if restoring {
                "还原备份"
            } else {
                "新建备份"
            })
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_resizable(true);
        if preview::active() {
            builder = builder.with_active(false);
        }
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            draft.owner_rect = ui.input(|input| input.viewport().inner_rect);
            let blocked =
                draft.confirm_restore || draft.complete.is_some() || draft.notice.is_some();
            if blocked {
                ui.disable();
                if ui.input(|input| input.viewport().close_requested()) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
            }
            action = if restoring {
                restore_view::render(ui, &mut draft, directory)
            } else {
                view::render(ui, &mut draft, &mut self.icons)
            };
            if blocked {
                action = None;
            }
            preview::capture(ui, viewport, &draft);
        });
        if draft.confirm_restore && !matches!(action, Some(view::Action::Close)) {
            use crate::native_dialog::confirmation::{self, Answer, Question};
            let header = format!(
                "确定要还原备份 \"{}\" 吗？",
                draft.restore.as_deref().unwrap_or_default()
            );
            let content = format!(
                "此操作将覆盖数据库 \"{}\" 中已存在的同名对象！",
                draft.scope.database
            );
            let question = Question {
                id: egui::ViewportId::from_hash_of(("restore-confirmation", &draft.id)),
                owner: draft.owner_rect,
                title: "确认还原",
                header: &header,
                content: &content,
                active: !preview::active(),
            };
            let object_count = draft.total();
            if let Some(answer) = confirmation::show(
                ctx,
                question,
                &mut draft.confirm_focus,
                &mut self.icons,
                |ui, id, bounds| preview::capture_question(ui, id, bounds, object_count),
            ) {
                draft.confirm_restore = false;
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
                if matches!(answer, Answer::Yes) {
                    action = Some(view::Action::ConfirmRestore);
                }
            }
        }
        if !matches!(action, Some(view::Action::Close))
            && alerts::show(ctx, &mut draft, &mut self.icons)
        {
            action = Some(view::Action::Close);
        }
        match action {
            Some(view::Action::Close) => {
                draft.release(ctx);
                return;
            }
            Some(view::Action::Start) if !draft.loading && !draft.running => {
                if restoring {
                    if draft.restore.is_some() {
                        draft.confirm_restore = true;
                        draft.confirm_focus = true;
                    }
                } else if let Err(error) = self.start_backup(&mut draft) {
                    draft.error = Some(format!("错误: {error:#}"));
                    draft.notice = draft.error.clone();
                }
            }
            Some(view::Action::ConfirmRestore) if !draft.running => {
                if let Err(error) = self.start_backup(&mut draft) {
                    draft.error = Some(format!("错误: {error:#}"));
                    draft.notice = draft.error.clone();
                }
            }
            Some(view::Action::RestoreSource(choice))
                if draft.choose_restore_source && !draft.running =>
            {
                use restore_source::Choice;
                match choice {
                    Choice::Archive(name) => draft.restore = Some(name),
                    Choice::OpenDirectory(name) => match draft.scope.child(&name) {
                        Ok(scope) => {
                            draft.scope = scope;
                            draft.restore = None;
                            self.load_restore_source(&mut draft);
                        }
                        Err(error) => draft.notice = Some(error.to_string()),
                    },
                    Choice::Parent => {
                        draft.scope.path = draft
                            .scope
                            .path
                            .rsplit_once('/')
                            .map_or_else(String::new, |(parent, _)| parent.to_owned());
                        draft.restore = None;
                        self.load_restore_source(&mut draft);
                    }
                    Choice::Reload => self.load_restore_source(&mut draft),
                }
            }
            Some(view::Action::SaveTemplate) => {
                let mut settings = self.settings.clone();
                match serde_json::to_value(template::Template::capture(&draft))
                    .map_err(anyhow::Error::from)
                    .and_then(|value| {
                        settings.extra.insert(TEMPLATE.into(), value);
                        self.preferences.save(&settings)
                    }) {
                    Ok(()) => {
                        self.settings = settings;
                        self.status = "模板已保存".into();
                        draft.notice = Some("模板已保存".into());
                    }
                    Err(error) => {
                        draft.error = Some(format!("保存模板失败：{error:#}"));
                        draft.tab = 3;
                    }
                }
            }
            Some(view::Action::LoadTemplate) => {
                match self
                    .settings
                    .extra
                    .get(TEMPLATE)
                    .cloned()
                    .map(serde_json::from_value::<template::Template>)
                {
                    Some(Ok(template)) => {
                        for editor in &draft.editors {
                            editor.release(ctx);
                        }
                        draft.editors = Default::default();
                        template.apply(&mut draft);
                    }
                    Some(Err(error)) => {
                        draft.error = Some(format!("加载模板失败：{error}"));
                        draft.tab = 3;
                    }
                    None => {
                        draft.notice = Some("暂无保存的模板".into());
                    }
                }
            }
            Some(view::Action::ClearTemplate) => {
                let mut settings = self.settings.clone();
                settings.extra.remove(TEMPLATE);
                match self.preferences.save(&settings) {
                    Ok(()) => {
                        self.settings = settings;
                        self.status = "模板已清除".into();
                        draft.notice = Some("模板已清除".into());
                    }
                    Err(error) => {
                        draft.error = Some(format!("清除模板失败：{error:#}"));
                        draft.tab = 3;
                    }
                }
            }
            _ => {}
        }
        self.backup_dialog = Some(draft);
    }
    fn start_backup(&mut self, draft: &mut Draft) -> anyhow::Result<()> {
        let objects = draft.objects();
        anyhow::ensure!(
            draft.restore.is_some() || !objects.is_empty(),
            "请至少选择一个备份对象"
        );
        let root = self
            .repository
            .location()
            .and_then(|path| path.parent())
            .ok_or_else(|| anyhow::anyhow!("当前工作区没有备份保存目录"))?
            .to_path_buf();
        let filename = if draft.options.custom_filename && !draft.options.filename.is_empty() {
            draft.options.filename.clone()
        } else {
            chrono::Local::now().format("%Y%m%d%H%M%S").to_string()
        };
        let filename = format!(
            "{}.nb3",
            filename
                .chars()
                .map(
                    |ch| if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                        '_'
                    } else {
                        ch
                    }
                )
                .collect::<String>()
        );
        let repaint = self.context.clone();
        let control = tomato_services::backup::Control::new(move || repaint.request_repaint());
        if draft.restore.is_some() {
            control.log("正在准备还原备份...");
        } else {
            control.start(objects.len() as u64);
            control.log(&format!("正在准备备份，共 {} 个对象...", objects.len()));
        }
        let command = Command::BackupDatabase {
            connection: draft.connection.clone(),
            root,
            control: Some(control.clone()),
            operation: if let Some(name) = &draft.restore {
                tomato_core::backup::Operation::Restore(tomato_core::backup::Restore {
                    scope: draft.scope.clone(),
                    name: name.clone(),
                    namespace: None,
                })
            } else {
                tomato_core::backup::Operation::Create(tomato_core::backup::Create {
                    scope: draft.scope.clone(),
                    filename,
                    objects,
                    comment: draft.options.comment.clone(),
                    lock_tables: draft.options.lock_tables,
                    single_transaction: draft.options.single_transaction,
                })
            },
        };
        let id = self.queue.submit(command)?;
        self.pending.insert(
            id,
            Pending {
                tab_id: draft.id.clone(),
                purpose: Purpose::Backup(draft.id.clone(), Job::Create),
            },
        );
        draft.control = control;
        draft.running = true;
        draft.started = Some(std::time::Instant::now());
        draft.elapsed_ms = None;
        draft.error = None;
        draft.complete = None;
        if draft.restore.is_none() {
            draft.tab = 3;
        }
        Ok(())
    }
}
