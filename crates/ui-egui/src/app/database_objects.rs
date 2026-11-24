//! Object-window use cases share the query actor and database identity.
use super::*;
pub(super) mod loading;
use crate::tabs::database_objects::window::{Action, Draft};
use tomato_core::{
    database_objects::{ChangeReport, Mutation, Request, Target},
    workbench::{DatabaseAction, DatabaseOutput, TableInfo},
};

pub(super) struct MutationJob {
    pub(super) connection: Connection,
    pub(super) request: Request,
}

impl TomatoApp {
    pub(super) fn new_scoped_query(&mut self, connection: Connection, schema: String) {
        let scope = tomato_core::queries::Scope {
            connection: connection.name.clone(),
            database: tabs::workbench::query_database(&connection, &schema).into(),
            path: String::new(),
        };
        let key = query_tree::identity::key(&connection, &schema);
        if let Err(error) = self.open_query_file(&connection.id, &key, scope, None) {
            self.error = Some(error.to_string());
        }
    }
    pub(super) fn disconnect_sql(&mut self, connection: &str) {
        let keep_queries = self.sql_hosts.get(connection).is_some_and(|h| {
            matches!(
                h.connection.kind(),
                "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
            )
        });
        let tabs = self.tabs.iter().filter(|tab| matches!(&tab.panel, Panel::Sql { connection: other, workbench, .. } if other.id == connection && (!keep_queries || workbench.table.is_some() || workbench.designer.is_some() || workbench.browser.is_some()))).collect::<Vec<_>>();
        if tabs.iter().any(|tab| tab.dirty()) {
            self.error = Some("此连接还有未保存的内容，请先保存或取消修改后关闭连接".into());
            return;
        }
        let ids = tabs.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>();
        match self.queue.submit(Command::Disconnect {
            id: connection.into(),
        }) {
            Ok(task) => {
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: format!("disconnect:{connection}"),
                        purpose: Purpose::SqlDisconnect(connection.into()),
                    },
                );
            }
            Err(error) => {
                self.error = Some(format!("关闭数据库连接失败: {error}"));
                return;
            }
        }
        for id in ids {
            self.close_tab(&id, false);
        }
        self.sql_selection.clear();
        self.sql_anchor = None;
        self.sql_hosts.remove(connection);
    }
    pub(super) fn open_objects(&mut self, connection: Connection, schema: String) {
        let connection = tabs::workbench::query_connection(&connection, &schema);
        let source = format!(
            "objects:{}:{}",
            connection.id,
            serde_json::to_string(&(connection.text("database"), &schema)).unwrap_or_default()
        );
        if let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let result = (|| -> anyhow::Result<Tab> {
            let mut tab = tabs::create_tab(
                connection.clone(),
                self.runtime.as_ref().map(|r| r.handle()),
            )?;
            tab.source_id = source;
            tab.title = format!(
                "对象@{}{}({}:{})",
                tabs::workbench::query_database(&connection, &schema),
                if connection.kind() == "POSTGRESQL" {
                    format!("/{schema}")
                } else {
                    String::new()
                },
                connection.text("host"),
                connection.number("port")
            );
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                workbench.schema = schema.clone();
                workbench.browser = Some(Box::new(Draft::default()));
            }
            Ok(tab)
        })();
        match result {
            Ok(tab) => {
                let id = tab.id.clone();
                self.tabs.push(tab);
                self.active = Some(id.clone());
                self.submit(
                    &id,
                    Command::Database {
                        connection,
                        action: DatabaseAction::Objects { schema },
                    },
                    Purpose::Normal,
                )
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub(super) fn object_action(&mut self, id: &str, action: Action) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.busy {
            return;
        }
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        let Some(browser) = &mut workbench.browser else {
            return;
        };
        let connection = connection.clone();
        let schema = workbench.schema.clone();
        let mut request = None;
        match action {
            Action::Open(index) => {
                let Some(object) = browser.catalog.objects.get(index) else {
                    return;
                };
                let table = TableInfo {
                    table: object.table.clone(),
                    view: object.view,
                };
                self.open_table(connection, table, false);
                return;
            }
            Action::OpenSelected { structure } => {
                let tables = browser
                    .selection_order
                    .iter()
                    .filter_map(|index| browser.catalog.objects.get(*index))
                    .map(|object| TableInfo {
                        table: object.table.clone(),
                        view: object.view,
                    })
                    .collect::<Vec<_>>();
                if tables.is_empty() {
                    return;
                }
                for table in tables {
                    self.activate_object_tab(connection.clone(), table, structure);
                }
                if let Some(id) = self.active.clone() {
                    self.load_object_tab(&id);
                }
                return;
            }
            Action::NewTable => {
                self.new_table(connection, schema);
                return;
            }
            Action::NewQuery => {
                self.new_scoped_query(connection, schema);
                return;
            }
            Action::Export => {
                if matches!(connection.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE") {
                    let database = tabs::workbench::query_database(&connection, &schema).to_owned();
                    let namespace = (connection.kind() == "POSTGRESQL").then_some(schema);
                    self.open_backup_in(connection, database, String::new(), namespace);
                    return;
                }
                if browser.selected.len() != 1 {
                    browser.notice = Some("请选择一张表或视图后导出".into());
                    return;
                }
                let table = browser
                    .selected
                    .iter()
                    .next()
                    .and_then(|i| browser.catalog.objects.get(*i))
                    .map(|object| object.table.clone());
                if let Some(table) = table {
                    self.open_object_data_export(connection, schema, table);
                }
                return;
            }
            Action::Import => {
                if matches!(connection.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE") {
                    let scope = tomato_core::backup::Scope {
                        connection: connection.name.clone(),
                        database: tabs::workbench::query_database(&connection, &schema).into(),
                        path: String::new(),
                    };
                    self.choose_backup_to_restore(connection, scope);
                    return;
                }
                let table = browser
                    .selected
                    .iter()
                    .next()
                    .and_then(|i| browser.catalog.objects.get(*i))
                    .map(|o| o.table.clone());
                self.open_data_import(connection, schema, table);
                return;
            }
            Action::Paste(text) => {
                if let Some(text) = text {
                    match super::table_transfer::Selection::parse(&text) {
                        Ok(_selection) => {
                            if let Err(error) = self.table_clipboard_port.write(&text) {
                                self.error = Some(error.to_string());
                                return;
                            }
                        }
                        Err(error) => {
                            self.error = Some(error.to_string());
                            return;
                        }
                    }
                }
                self.paste_tables(connection, schema);
                return;
            }
            Action::Copy => {
                if let Some(text) = browser.copy_tables(
                    &connection.id,
                    tabs::workbench::query_database(&connection, &schema),
                    if connection.kind() == "POSTGRESQL" {
                        &schema
                    } else {
                        ""
                    },
                ) && let Ok(selection) = super::table_transfer::Selection::parse(&text)
                {
                    self.copy_table_selection(&connection, &schema, selection.tables);
                }
                return;
            }
            Action::Mutate(confirmed) => {
                request = Some(confirmed);
            }
            Action::Rename(index, name) => {
                let name = name.trim_matches(|ch: char| ch <= ' ');
                let Some(object) = browser.catalog.objects.get(index) else {
                    return;
                };
                if name.is_empty() || name == object.table.name {
                    return;
                }
                request = Some(Request {
                    targets: vec![Target {
                        table: object.table.clone(),
                        view: object.view,
                    }],
                    mutation: Mutation::Rename { name: name.into() },
                });
            }
            Action::Refresh => {}
        }
        let (action, purpose) = if let Some(request) = request {
            let job = MutationJob {
                connection: connection.clone(),
                request: request.clone(),
            };
            (
                DatabaseAction::ObjectMutation { schema, request },
                Purpose::ObjectMutation(Box::new(job)),
            )
        } else {
            (DatabaseAction::Objects { schema }, Purpose::Normal)
        };
        self.submit(id, Command::Database { connection, action }, purpose);
    }
    pub(super) fn object_mutation_reply(
        &mut self,
        id: &str,
        result: anyhow::Result<Output>,
    ) -> Option<ChangeReport> {
        let owner = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .and_then(|tab| {
                let Panel::Sql { workbench, .. } = &mut tab.panel else {
                    return None;
                };
                tab.busy = false;
                tab.error = None;
                Some(workbench)
            });
        match result {
            Ok(Output::Database(DatabaseOutput::ObjectsChanged(report))) => {
                if let Some(owner) = owner {
                    owner.record_object_report(&report);
                } else if let Some(error) = failed_objects(&report) {
                    self.error = Some(error);
                }
                Some(report)
            }
            other => {
                let message = match other {
                    Err(error) => {
                        if let Some(failure) =
                            error.downcast_ref::<tomato_services::database::ConnectionFailure>()
                            && failure.is_restored()
                        {
                            self.status = failure.to_string();
                            return None;
                        }
                        format!("{error:#}")
                    }
                    Ok(_) => "表操作任务返回类型错误".into(),
                };
                if let Some(browser) = owner.and_then(|workbench| workbench.browser.as_mut()) {
                    browser.notice = Some(message);
                } else {
                    self.error = Some(message);
                }
                None
            }
        }
    }
    pub(super) fn receive_object_changes(
        &mut self,
        owner: &str,
        connection: &Connection,
        request: &Request,
        report: &ChangeReport,
    ) {
        let Some(target) = request.targets.first() else {
            return;
        };
        let schema = &target.table.schema;
        let same_database = tabs::workbench::session_database(connection, schema);
        let succeeded = report
            .succeeded
            .iter()
            .map(|table| table.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let data_changed = matches!(request.mutation, Mutation::Clear | Mutation::Truncate);
        self.update_object_tree(connection, &request.mutation, schema, &succeeded);
        let mut refresh = Vec::new();
        for tab in &mut self.tabs {
            if tab.id == owner {
                continue;
            }
            let Panel::Sql {
                connection: other,
                workbench,
                ..
            } = &mut tab.panel
            else {
                continue;
            };
            if other.id != connection.id
                || tabs::workbench::session_database(other, &workbench.schema) != same_database
                || &workbench.schema != schema
                || succeeded.is_empty()
            {
                continue;
            }
            let action = if workbench.browser.is_some() {
                // Refresh only idle windows: never replace a pending mutation's selection.
                (!tab.busy).then(|| DatabaseAction::Objects {
                    schema: schema.clone(),
                })
            } else if let Some(table) = &workbench.table {
                if &table.schema != schema || !succeeded.contains(table.name.as_str()) {
                    continue;
                }
                if !data_changed {
                    workbench.needs_refresh = true;
                    workbench.status = "数据库对象已删除或重命名，当前标签内容已保留".into();
                    continue;
                }
                if workbench.object_load_pending || workbench.designer.is_some() {
                    continue;
                }
                if tab.busy || workbench.dirty() {
                    workbench.needs_refresh = true;
                    workbench.status = "表数据已清空或截断，请处理未保存的数据后刷新".into();
                    continue;
                }
                workbench.request().map(|mut request| {
                    request.offset = 0;
                    DatabaseAction::Page { request }
                })
            } else {
                // Changing row data does not invalidate SQL completion metadata.
                (!data_changed).then(|| DatabaseAction::Completion {
                    schema: schema.clone(),
                })
            };
            if let Some(action) = action {
                refresh.push((tab.id.clone(), other.clone(), action));
            }
        }
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == owner)
            && let Panel::Sql { workbench, .. } = &mut tab.panel
        {
            if let Some(browser) = &mut workbench.browser {
                browser.notice = failed_objects(report);
            }
            refresh.push((
                tab.id.clone(),
                connection.clone(),
                DatabaseAction::Objects {
                    schema: workbench.schema.clone(),
                },
            ));
        }
        for (id, connection, action) in refresh {
            self.submit(
                &id,
                Command::Database { connection, action },
                Purpose::Normal,
            );
        }
    }
}

fn failed_objects(report: &ChangeReport) -> Option<String> {
    (!report.failures.is_empty()).then(|| {
        report
            .failures
            .iter()
            .map(|(table, error)| format!("{}: {error}", table.name))
            .collect::<Vec<_>>()
            .join("\n")
    })
}
