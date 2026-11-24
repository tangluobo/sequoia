//! Database dialogs and confirmations share the existing connection actor.
use super::*;
use tomato_core::database_admin::{Info, Output as AdminOutput, Request};
mod preview;
#[cfg(test)]
mod tests;
mod view;
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Create,
    Edit,
    Drop,
}
pub(super) struct Draft {
    id: String,
    connection: Connection,
    mode: Mode,
    database: String,
    charset: String,
    collation: String,
    info: Info,
    page: usize,
    loading: bool,
    running: bool,
    focus: bool,
    error: Option<String>,
    owner: Option<egui::Rect>,
    editors: [crate::tabs::text_state::Editor; 2],
}
impl Draft {
    pub fn release(&self, ctx: &egui::Context) {
        for editor in &self.editors {
            editor.release(ctx);
        }
    }
    fn preview(&self) -> String {
        let sql = self
            .request()
            .sql(self.connection.kind())
            .ok()
            .flatten()
            .unwrap_or_default();
        format!("{sql};")
    }
    fn size(&self) -> [f32; 2] {
        if self.mode == Mode::Edit {
            [
                (self.preview().chars().count() as f32 * 7.8 + 48.)
                    .ceil()
                    .max(450.),
                284.,
            ]
        } else {
            [450., 289.]
        }
    }
    fn request(&self) -> Request {
        match self.mode {
            Mode::Drop => Request::Drop {
                database: self.database.clone(),
            },
            Mode::Create => Request::Create {
                database: self.database.trim_matches(|c: char| c <= ' ').into(),
                charset: self.charset.clone(),
                collation: self.collation.clone(),
            },
            Mode::Edit => Request::Alter {
                database: self.database.clone(),
                charset: self.charset.clone(),
                collation: self.collation.clone(),
            },
        }
    }
}
impl TomatoApp {
    pub(super) fn open_database_admin(
        &mut self,
        connection: Connection,
        database: String,
        mode: Mode,
    ) {
        let connection = self.live_sql_connection(connection);
        if self.sql_password_missing(&connection) {
            self.credential = Some(credentials::Prompt::then(
                connection,
                credentials::Resume::DatabaseAdmin { database, mode },
            ));
            return;
        }
        if mode==Mode::Drop&&self.tabs.iter().any(|tab|tab.dirty()&&matches!(&tab.panel,Panel::Sql{connection:c,workbench,..}if c.id==connection.id&&tabs::workbench::query_database(c,&workbench.schema)==database&&(workbench.table.is_some()||workbench.browser.is_some()||workbench.designer.is_some()))){self.error=Some("此数据库还有未保存的内容，请先保存或取消修改".into());return;}
        if let Some(old) = self.database_admin.take() {
            old.release(&self.context);
        }
        let draft = Draft {
            id: Connection::new("TOOL").id,
            connection: connection.clone(),
            mode,
            database: database.clone(),
            charset: String::new(),
            collation: String::new(),
            info: Info::default(),
            page: 0,
            loading: false,
            running: false,
            focus: true,
            error: None,
            owner: None,
            editors: Default::default(),
        };
        self.database_admin = Some(draft);
        if mode != Mode::Drop {
            self.submit_database_admin(Request::Read {
                database: if mode == Mode::Edit {
                    Some(database)
                } else {
                    None
                },
            });
        }
    }
    fn submit_database_admin(&mut self, request: Request) {
        let Some(draft) = &mut self.database_admin else {
            return;
        };
        draft.loading = true;
        draft.running = !matches!(request, Request::Read { .. });
        match self.queue.submit(Command::DatabaseAdmin {
            connection: draft.connection.clone(),
            request,
        }) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: draft.id.clone(),
                        purpose: Purpose::DatabaseAdmin(draft.id.clone()),
                    },
                );
            }
            Err(error) => {
                draft.loading = false;
                draft.running = false;
                draft.error = Some(error.to_string());
            }
        }
    }
    pub(super) fn database_admin_reply(&mut self, id: &str, result: anyhow::Result<Output>) {
        let Some(draft) = self.database_admin.as_mut().filter(|d| d.id == id) else {
            return;
        };
        draft.loading = false;
        draft.running = false;
        match result {
            Ok(Output::DatabaseAdmin(AdminOutput::Info(info))) => {
                draft.charset = info.charset.clone();
                draft.collation = info.collation.clone();
                draft.info = info;
            }
            Ok(Output::DatabaseAdmin(AdminOutput::Changed { database, dropped })) => {
                let connection = draft.connection.clone();
                draft.release(&self.context);
                self.database_admin = None;
                if dropped {
                    let ids=self.tabs.iter().filter(|tab|matches!(&tab.panel,Panel::Sql{connection:c,workbench,..}if c.id==connection.id&&tabs::workbench::query_database(c,&workbench.schema)==database&&(workbench.table.is_some()||workbench.browser.is_some()||workbench.designer.is_some()))).map(|tab|tab.id.clone()).collect::<Vec<_>>();
                    for id in ids {
                        self.close_tab(&id, false);
                    }
                    if let Some(pg) = self
                        .sql_hosts
                        .get_mut(&connection.id)
                        .and_then(|h| h.pg.as_mut())
                    {
                        pg.databases.remove(&database);
                    }
                    if let Some(host) = self.sql_hosts.get_mut(&connection.id) {
                        host.tables.remove(&database);
                        host.transactions.remove(&database);
                        if let Some(tree) = &mut host.direct {
                            tree.scopes.remove(&database);
                        }
                    }
                }
                if dropped {
                    for tab in &mut self.tabs {
                        if let Panel::Sql {
                            connection: c,
                            workbench,
                            ..
                        } = &mut tab.panel
                            && c.id == connection.id
                            && tabs::workbench::query_database(c, &workbench.schema) == database
                        {
                            workbench.transaction = false;
                            workbench.selectors.selected = false;
                            workbench.status = "数据库已删除，请选择其他数据库".into();
                        }
                    }
                }
                if connection.kind() == "POSTGRESQL" {
                    self.open_pg_host(connection, true);
                } else {
                    self.refresh_mysql_databases(connection);
                }
            }
            Err(error) => draft.error = Some(error.to_string()),
            _ => draft.error = Some("数据库管理任务返回类型错误".into()),
        }
    }
    fn refresh_mysql_databases(&mut self, connection: Connection) {
        self.open_direct_host(connection, true);
    }
    pub(super) fn database_admin_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.database_admin.take() else {
            return;
        };
        let mut action = None;
        if draft.mode == Mode::Drop && !draft.running && draft.error.is_none() {
            let question = crate::native_dialog::confirmation::Question {
                id: egui::ViewportId::from_hash_of(("drop-database", &draft.id)),
                owner: draft.owner,
                title: "删除数据库",
                header: &format!("确定要删除数据库 \"{}\" 吗？", draft.database),
                content: "此操作不可撤销，该数据库中的所有数据将被永久删除！",
                active: true,
            };
            if let Some(answer) = crate::native_dialog::confirmation::show(
                ctx,
                question,
                &mut draft.focus,
                &mut self.icons,
                |_, _, _| {},
            ) {
                action = Some(
                    if matches!(answer, crate::native_dialog::confirmation::Answer::Yes) {
                        view::Action::Submit
                    } else {
                        view::Action::Close
                    },
                );
            }
        } else {
            let title = match draft.mode {
                Mode::Create => "新建数据库".into(),
                Mode::Edit => format!("编辑数据库 - {}", draft.database),
                Mode::Drop => "删除数据库".into(),
            };
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("database-admin", &draft.id)),
                egui::ViewportBuilder::default()
                    .with_title(title)
                    .with_inner_size(draft.size())
                    .with_active(!preview::active())
                    .with_resizable(false),
                |ui, _| {
                    draft.owner = ui.input(|i| i.viewport().inner_rect);
                    action = view::render(ui, &mut draft);
                    preview::capture(ui);
                },
            );
        }
        if matches!(action, Some(view::Action::Close)) {
            draft.release(ctx);
            return;
        }
        let request = matches!(action, Some(view::Action::Submit)).then(|| draft.request());
        self.database_admin = Some(draft);
        if let Some(request) = request {
            if let Err(error) = request.sql(self.database_admin.as_ref().unwrap().connection.kind())
            {
                self.database_admin.as_mut().unwrap().error = Some(error.to_string());
            } else {
                self.submit_database_admin(request);
            }
        }
    }
}
