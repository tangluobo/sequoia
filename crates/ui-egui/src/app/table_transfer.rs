//! Original two-step copy-table wizard; the controller owns admission and refresh.
use super::*;
use tomato_core::{
    table_transfer::{Plan, Request},
    workbench::{DatabaseAction, DatabaseOutput},
};
#[cfg(test)]
mod controls;
mod preview;
#[cfg(test)]
mod tests;
mod view;
#[derive(Clone)]
pub(super) struct Selection {
    pub connection: String,
    pub database: String,
    pub schema: String,
    pub tables: Vec<String>,
}
impl Selection {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(text.len() <= 1024 * 1024, "表复制剪贴板超过1 MiB");
        let mut lines = text.lines();
        anyhow::ensure!(
            lines.next() == Some("TOMATO_COPY_TABLES"),
            "剪贴板中没有复制的表"
        );
        let connection = lines.next().unwrap_or_default().into();
        let database = lines.next().unwrap_or_default().into();
        let schema = lines.next().unwrap_or_default().into();
        let tables = lines
            .filter(|name| !name.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            !tables.is_empty() && tables.len() <= 1000,
            "请选择1至1,000张表"
        );
        Ok(Self {
            connection,
            database,
            schema,
            tables,
        })
    }
}
pub(super) enum Job {
    Databases,
    Tables,
    Preview,
    Execute(Box<Refresh>),
}
pub(super) struct Refresh {
    connection: Connection,
    schema: String,
}
pub(super) struct PasteNotice {
    text: String,
    failed: bool,
    focus: bool,
}
pub(super) struct Draft {
    id: String,
    generation: u64,
    request: Request,
    databases: Vec<String>,
    tables: Vec<(String, bool)>,
    step: usize,
    loading: bool,
    running: bool,
    completed: bool,
    plan: Option<Plan>,
    preview: String,
    error: Option<String>,
    log: String,
    control: tomato_services::backup::Control,
    options: bool,
    option_values: [bool; 3],
    editors: [crate::tabs::text_state::Editor; 2],
    layout: view::Layout,
    scroll_areas: [Option<egui::Id>; 2],
}
impl Draft {
    fn target_database(&self) -> &str {
        if self.request.target.kind() == "ORACLE" {
            &self.request.target_schema
        } else {
            self.request.target.text("database")
        }
    }
    fn set_target_database(&mut self, database: String) {
        if self.request.target.kind() == "ORACLE" {
            self.request
                .target
                .set("__tomatoOracleOwner", database.clone());
            self.request.target_schema = database;
        } else {
            self.request.target.set("database", database.clone());
            if self.request.target.kind() == "MYSQL" {
                self.request.target_schema = database;
            }
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            for id in self.scroll_areas.into_iter().flatten() {
                data.remove::<egui::scroll_area::State>(id);
            }
        });

        self.control.cancel();
        for editor in &self.editors {
            editor.release(ctx);
        }
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.plan = None;
        self.preview.clear();
        self.error = None;
        self.completed = false;
    }
}
impl TomatoApp {
    pub(super) fn copy_table_selection(
        &mut self,
        connection: &Connection,
        schema: &str,
        tables: Vec<String>,
    ) {
        if tables.is_empty() {
            return;
        }
        let database = tabs::workbench::query_database(connection, schema).to_owned();
        let mut text = format!(
            "TOMATO_COPY_TABLES\n{}\n{}\n{}",
            connection.id,
            database,
            if matches!(connection.kind(), "POSTGRESQL" | "ORACLE") {
                schema
            } else {
                ""
            }
        );
        for name in &tables {
            text.push('\n');
            text.push_str(name);
        }
        if let Err(error) = self.table_clipboard_port.write(&text) {
            self.error = Some(error.to_string());
        }
    }
    pub(super) fn paste_tables(&mut self, target: Connection, schema: String) {
        let selection = match self
            .table_clipboard_port
            .read()
            .and_then(|text| Selection::parse(&text))
        {
            Ok(selection) => selection,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let direct = selection.connection == target.id;
        self.open_table_transfer(selection, target, schema, direct);
    }
    pub(super) fn start_copy_table(
        &mut self,
        connection: Connection,
        table: tomato_core::workbench::TableId,
    ) {
        let selection = Selection {
            connection: connection.id.clone(),
            database: tabs::workbench::query_database(&connection, &table.schema).into(),
            schema: table.schema.clone(),
            tables: vec![table.name],
        };
        self.open_table_transfer(selection, connection, table.schema, false);
    }
    fn open_table_transfer(
        &mut self,
        selection: Selection,
        target: Connection,
        schema: String,
        direct: bool,
    ) {
        let Some(mut source) = self
            .store
            .connections
            .iter()
            .find(|c| c.id == selection.connection)
            .cloned()
        else {
            self.error = Some("复制表的源连接已不存在".into());
            return;
        };
        source = self.live_sql_connection(source);
        source = tomato_core::database_scope::select(&source, &selection.database);
        let source_schema = if source.kind() == "MYSQL" {
            selection.database
        } else {
            selection.schema
        };
        let target = tabs::workbench::query_connection(&self.live_sql_connection(target), &schema);
        let request = Request {
            source,
            target,
            source_schema,
            target_schema: schema,
            tables: selection.tables,
            structure: true,
            data: true,
            drop_existing: false,
            add: true,
            drop: true,
            modify: true,
        };
        if direct {
            self.start_direct_paste(request);
            return;
        }
        if let Some(old) = self.table_transfer.take() {
            old.release(&self.context);
        }
        let ctx = self.context.clone();
        let draft = Draft {
            id: Connection::new("TOOL").id,
            generation: 0,
            request,
            databases: Vec::new(),
            tables: Vec::new(),
            step: 0,
            loading: false,
            running: false,
            completed: false,
            plan: None,
            preview: String::new(),
            error: None,
            log: String::new(),
            control: tomato_services::backup::Control::new(move || ctx.request_repaint()),
            options: false,
            option_values: [true, true, false],
            editors: Default::default(),
            layout: Default::default(),
            scroll_areas: [None; 2],
        };
        self.table_transfer = Some(draft);
        self.transfer_databases();
    }
    pub(super) fn start_direct_paste(&mut self, request: Request) {
        if self.table_paste.is_some() {
            return;
        }
        if self.sql_password_missing(&request.source) {
            self.credential = Some(credentials::Prompt::then(
                request.source.clone(),
                credentials::Resume::PasteTables(Box::new(request)),
            ));
            return;
        }
        let ctx = self.context.clone();
        let control = tomato_services::backup::Control::new(move || ctx.request_repaint());
        let refresh = Refresh {
            connection: request.target.clone(),
            schema: request.target_schema.clone(),
        };
        match self.queue.submit(Command::PasteTables {
            request: Box::new(request),
            control: Some(control.clone()),
        }) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::TablePaste(Box::new(refresh)),
                    },
                );
                self.table_paste = Some(control);
                self.status = "正在粘贴表…".into();
            }
            Err(error) => self.table_paste_result(refresh, Err(error)),
        }
    }
    pub(super) fn table_paste_result(&mut self, refresh: Refresh, result: anyhow::Result<Output>) {
        self.table_paste = None;
        let result = result.and_then(|output| {
            let Output::TransferReport(report) = output else {
                anyhow::bail!("表粘贴任务返回类型错误");
            };
            let mut touched = report.succeeded;
            touched.extend(report.failed.iter().map(|(table, _)| table.clone()));
            if !touched.is_empty() {
                self.refresh_transfer_dependents(&refresh.connection, &refresh.schema, &touched);
            }
            anyhow::ensure!(!report.cancelled, "操作已取消");
            if let Some((table, error)) = report.failed.first() {
                anyhow::bail!(
                    "{}：{}（成功 {} 张，失败 {} 张）",
                    table.name,
                    error,
                    touched.len() - report.failed.len(),
                    report.failed.len()
                );
            }
            Ok(())
        });
        let (text, failed) = match result {
            Ok(()) => ("表粘贴成功！".into(), false),
            Err(error) => (format!("表粘贴失败: {error:#}"), true),
        };
        self.status = text.clone();
        self.table_paste_notice = Some(PasteNotice {
            text,
            failed,
            focus: true,
        });
    }
    pub(super) fn table_paste_dialog(&mut self, ctx: &egui::Context) {
        use crate::native_dialog::message::{self, Button, Kind, Message, Role};
        let Some(notice) = &mut self.table_paste_notice else {
            return;
        };
        if message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of("table-paste-result"),
                owner: ctx.input(|i| i.viewport().outer_rect),
                title: "粘贴表",
                header: None,
                content: &notice.text,
                kind: if notice.failed {
                    Kind::Error
                } else {
                    Kind::Information
                },
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            &mut notice.focus,
            &mut self.icons,
            |_, _| {},
        )
        .is_some()
        {
            self.table_paste_notice = None;
        }
    }
    fn transfer_submit(&mut self, command: Command, job: Job) {
        let Some(draft) = &mut self.table_transfer else {
            return;
        };
        draft.loading = true;
        match self.queue.submit(command) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: draft.id.clone(),
                        purpose: Purpose::TableTransfer(draft.id.clone(), draft.generation, job),
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
    fn transfer_databases(&mut self) {
        let Some(draft) = &self.table_transfer else {
            return;
        };
        if self.sql_password_missing(&draft.request.target) {
            self.credential = Some(credentials::Prompt::then(
                draft.request.target.clone(),
                credentials::Resume::Transfer {
                    id: draft.id.clone(),
                    generation: draft.generation,
                    source: false,
                },
            ));
            return;
        }
        self.transfer_submit(
            Command::Database {
                connection: draft.request.target.clone(),
                action: DatabaseAction::Databases,
            },
            Job::Databases,
        );
    }
    fn transfer_tables(&mut self) {
        let Some(draft) = &self.table_transfer else {
            return;
        };
        if self.sql_password_missing(&draft.request.source) {
            self.credential = Some(credentials::Prompt::then(
                draft.request.source.clone(),
                credentials::Resume::Transfer {
                    id: draft.id.clone(),
                    generation: draft.generation,
                    source: true,
                },
            ));
            return;
        }
        let command = Command::Database {
            connection: draft.request.source.clone(),
            action: DatabaseAction::Tables {
                schema: draft.request.source_schema.clone(),
            },
        };
        self.transfer_submit(command, Job::Tables);
    }
    pub(super) fn resume_transfer_credentials(
        &mut self,
        id: &str,
        generation: u64,
        connection: Connection,
        source: bool,
    ) {
        let Some(draft) = self
            .table_transfer
            .as_mut()
            .filter(|draft| draft.id == id && draft.generation == generation)
        else {
            return;
        };
        let role = if source {
            &mut draft.request.source
        } else {
            &mut draft.request.target
        };
        if role.id != connection.id {
            return;
        }
        role.set("password", connection.text("password"));
        if source && draft.request.target.id == connection.id {
            draft
                .request
                .target
                .set("password", connection.text("password"));
        }
        if !source && draft.request.source.id == connection.id {
            draft
                .request
                .source
                .set("password", connection.text("password"));
        }
        if source {
            self.transfer_tables();
        } else {
            self.transfer_databases();
        }
    }
    fn transfer_preview(&mut self) {
        let Some(draft) = &mut self.table_transfer else {
            return;
        };
        draft.invalidate();
        draft.request.tables = draft
            .tables
            .iter()
            .filter(|(_, selected)| *selected)
            .map(|(name, _)| name.clone())
            .collect();
        if draft.request.tables.is_empty() {
            return;
        }
        let command = Command::TransferTables {
            request: Box::new(draft.request.clone()),
            accepted: None,
            control: Some(draft.control.clone()),
        };
        self.transfer_submit(command, Job::Preview);
    }
    pub(super) fn table_transfer_reply(
        &mut self,
        id: &str,
        generation: u64,
        job: Job,
        result: anyhow::Result<Output>,
    ) {
        let Some(draft) = self
            .table_transfer
            .as_mut()
            .filter(|d| d.id == id && d.generation == generation)
        else {
            if let (Job::Execute(scope), Ok(Output::TransferReport(report))) = (job, result) {
                let mut tables = report.succeeded;
                tables.extend(report.failed.into_iter().map(|(table, _)| table));
                self.refresh_transfer_dependents(&scope.connection, &scope.schema, &tables);
            }
            return;
        };
        draft.loading = false;
        let mut preview = false;
        let mut changed = None;
        match (job, result) {
            (Job::Databases, Ok(Output::Database(DatabaseOutput::Databases(databases)))) => {
                if !databases.iter().any(|name| name == draft.target_database())
                    && let Some(first) = databases
                        .iter()
                        .find(|name| {
                            draft.request.target.kind() == "ORACLE"
                                && name.eq_ignore_ascii_case(draft.request.target.text("username"))
                        })
                        .or_else(|| databases.first())
                {
                    draft.set_target_database(first.clone());
                }
                draft.databases = databases;
            }
            (Job::Tables, Ok(Output::Database(DatabaseOutput::Tables(tables)))) => {
                draft.tables = tables
                    .into_iter()
                    .filter(|t| !t.view)
                    .map(|t| {
                        let selected = draft.request.tables.contains(&t.table.name);
                        (t.table.name, selected)
                    })
                    .collect();
                preview = true;
            }
            (Job::Preview, Ok(Output::TransferPlan(plan))) => {
                draft.preview = plan.preview();
                draft.plan = Some(plan);
            }
            (Job::Execute(_), Ok(Output::TransferReport(report))) => {
                draft.running = false;
                draft.completed = true;
                let message = format!(
                    "成功 {} 张，失败 {} 张，已提交 {} 行{}",
                    report.succeeded.len(),
                    report.failed.len(),
                    report.records,
                    if report.cancelled { "，已取消" } else { "" }
                );
                draft.control.log(&message);
                if !report.failed.is_empty() {
                    draft.error = Some(
                        report
                            .failed
                            .iter()
                            .map(|(table, error)| format!("{}: {error}", table.name))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
                let mut touched = report.succeeded;
                touched.extend(report.failed.into_iter().map(|(table, _)| table));
                changed = Some((
                    draft.request.target.clone(),
                    draft.request.target_schema.clone(),
                    touched,
                ));
            }
            (_, Err(error)) => {
                draft.loading = false;
                draft.running = false;
                draft.error = Some(error.to_string());
            }
            _ => {
                draft.running = false;
                draft.error = Some("表传输任务返回类型错误".into());
            }
        }
        if preview {
            self.transfer_preview();
        }
        if let Some((connection, schema, tables)) = changed {
            self.refresh_transfer_dependents(&connection, &schema, &tables);
        }
    }
    pub(super) fn refresh_transfer_dependents(
        &mut self,
        connection: &Connection,
        schema: &str,
        tables: &[tomato_core::workbench::TableId],
    ) {
        let mut jobs = Vec::new();
        for tab in &mut self.tabs {
            let dirty = tab.dirty();
            let Panel::Sql {
                connection: other,
                workbench,
                ..
            } = &mut tab.panel
            else {
                continue;
            };
            if other.id != connection.id
                || workbench.object_load_pending
                || tabs::workbench::session_database(other, &workbench.schema)
                    != tabs::workbench::session_database(connection, schema)
            {
                continue;
            }
            if tab.busy {
                workbench.needs_refresh = true;
                continue;
            }
            let action = if workbench.browser.is_some() {
                Some(DatabaseAction::Objects {
                    schema: workbench.schema.clone(),
                })
            } else if let Some(table) = workbench
                .table
                .as_ref()
                .filter(|table| tables.contains(table))
            {
                if dirty {
                    workbench.needs_refresh = true;
                    workbench.status = "表数据传输已完成，请处理未保存内容后刷新".into();
                    None
                } else if workbench.designer.is_some() {
                    Some(DatabaseAction::Design {
                        table: table.clone(),
                    })
                } else {
                    workbench.request().map(|mut request| {
                        request.offset = 0;
                        request.sort = None;
                        DatabaseAction::Page { request }
                    })
                }
            } else if workbench.table.is_none() {
                Some(DatabaseAction::Completion {
                    schema: workbench.schema.clone(),
                })
            } else {
                None
            };
            if let Some(action) = action {
                jobs.push((tab.id.clone(), other.clone(), action));
            }
        }
        for (id, connection, action) in jobs {
            self.submit(
                &id,
                Command::Database { connection, action },
                Purpose::Normal,
            );
        }
        self.refresh_tree_tables(connection.clone(), schema.into());
    }
    pub(super) fn table_transfer_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.table_transfer.take() else {
            return;
        };
        let viewport = egui::ViewportId::from_hash_of(("table-transfer", &draft.id));
        let mut action = None;
        ctx.show_viewport_immediate(
            viewport,
            egui::ViewportBuilder::default()
                .with_title("数据传输")
                .with_inner_size([800., 600.])
                .with_min_inner_size([800., 600.])
                .with_active(!preview::active()),
            |ui, _| {
                action = view::render(ui, &mut draft, &self.store.connections, &mut self.icons);
                preview::capture(ui, viewport);
            },
        );
        view::options(ctx, &mut draft, !preview::active());
        let close = matches!(action, Some(view::Action::Close));
        if close {
            draft.release(ctx);
            return;
        }
        self.table_transfer = Some(draft);
        match action {
            Some(view::Action::Target(id)) => {
                let target = self.store.connections.iter().find(|c| c.id == id).cloned();
                if let Some(target) = target {
                    let target = self.live_sql_connection(target);
                    let draft = self.table_transfer.as_mut().unwrap();
                    draft.invalidate();
                    draft.request.target = target;
                    draft.request.target_schema = match draft.request.target.kind() {
                        "POSTGRESQL" => "public".into(),
                        "ORACLE" => draft.request.target.text("__tomatoOracleOwner").into(),
                        _ => draft.request.target.text("database").into(),
                    };
                    self.transfer_databases();
                }
            }
            Some(view::Action::Next) => {
                let draft = self.table_transfer.as_mut().unwrap();
                draft.step = 1;
                draft.invalidate();
                self.transfer_tables();
            }
            Some(view::Action::Preview) => self.transfer_preview(),
            Some(view::Action::Execute) => {
                let draft = self.table_transfer.as_mut().unwrap();
                if let Some(plan) = draft.plan.take() {
                    draft.running = true;
                    draft.error = None;
                    let command = Command::TransferTables {
                        request: Box::new(draft.request.clone()),
                        accepted: Some(plan),
                        control: Some(draft.control.clone()),
                    };
                    let scope = Refresh {
                        connection: draft.request.target.clone(),
                        schema: draft.request.target_schema.clone(),
                    };
                    self.transfer_submit(command, Job::Execute(Box::new(scope)));
                }
            }
            _ => {}
        }
    }
}
