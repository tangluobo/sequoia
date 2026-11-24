//! Designer commands and cross-tab invalidation; views never execute DDL.
use super::*;
use crate::tabs::table_design::{Draft, Name, Notice};
use tomato_core::workbench::{DatabaseAction, TableId};
impl TomatoApp {
    pub(super) fn new_table(&mut self, connection: Connection, schema: String) {
        let connection = tabs::workbench::query_connection(&connection, &schema);
        let result = (|| -> anyhow::Result<Tab> {
            let mut tab = tabs::create_tab(
                connection.clone(),
                self.runtime.as_ref().map(|r| r.handle()),
            )?;
            tab.source_id = format!("new-table:{}:{}", connection.id, tab.id);
            tab.title = format!(
                "新建表@{}({}:{})-表结构",
                tabs::workbench::query_database(&connection, &schema),
                connection.text("host"),
                connection.number("port")
            );
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                let mut draft = Draft::new(connection.kind(), schema.clone())?;
                draft.busy = true;
                workbench.schema = schema;
                workbench.designer = Some(Box::new(draft));
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
                        action: DatabaseAction::DesignMetadata,
                    },
                    Purpose::Normal,
                );
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub(super) fn save_design(&mut self, id: &str, name: Option<String>) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        let Some(draft) = workbench.designer.as_mut() else {
            return;
        };
        if tab.busy || draft.busy {
            return;
        }
        draft.commit_edit(&self.context);
        let errors = if draft.saved.is_none() {
            draft.request.validation_errors()
        } else {
            Vec::new()
        };
        if !errors.is_empty() {
            draft.notice = Some(Notice {
                title: "新建表".into(),
                header: Some("字段设置不完整，请检查以下问题".into()),
                text: errors.join("\n"),
                error: false,
                focus: true,
            });
            return;
        }
        if draft.saved.is_none()
            && draft
                .request
                .columns
                .iter()
                .all(|c| c.name.trim_matches(|ch: char| ch <= ' ').is_empty())
        {
            draft.status = "请先添加至少一个字段".into();
            return;
        }
        let action = if let Some(edit) = draft.edit_request() {
            match edit.plan(draft.dialect, draft.backslash_escapes()) {
                Ok(plan) if plan.statements.is_empty() => {
                    draft.status = if plan.reorder_unsupported {
                        "当前数据库类型不支持直接调整字段顺序"
                    } else {
                        "没有需要保存的变更"
                    }
                    .into();
                    return;
                }
                Err(e) => {
                    draft.status = e.to_string();
                    return;
                }
                _ => {}
            }
            DatabaseAction::SaveStructure {
                edit: Box::new(edit),
            }
        } else {
            let Some(name) = name else {
                draft.name = Some(Name {
                    text: String::new(),
                    focus: true,
                    editor: Default::default(),
                });
                return;
            };
            if name.is_empty() {
                draft.status = "表名不能为空".into();
                return;
            }
            draft.request.table.name = name;
            DatabaseAction::CreateTable {
                request: draft.snapshot(),
            }
        };
        draft.saving = true;
        draft.busy = true;
        draft.status = if draft.saved.is_some() {
            "正在保存变更..."
        } else {
            "正在创建表..."
        }
        .into();
        let connection = connection.clone();
        self.submit(
            id,
            Command::Database { connection, action },
            Purpose::Normal,
        );
    }
    pub(super) fn refresh_design(&mut self, id: &str) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        let Some(draft) = workbench.designer.as_mut() else {
            return;
        };
        if tab.busy || draft.busy {
            return;
        }
        draft.cancel_edit(&self.context);
        let action = if let Some(saved) = &draft.saved {
            DatabaseAction::DesignStructure {
                table: saved.definition.table.clone(),
            }
        } else {
            DatabaseAction::DesignMetadata
        };
        draft.busy = true;
        let connection = connection.clone();
        self.submit(
            id,
            Command::Database { connection, action },
            Purpose::Normal,
        );
    }
    pub(super) fn load_design_sql(&mut self, id: &str) {
        use crate::tabs::table_design::{Page, SqlDefinition};
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        let Some(draft) = workbench.designer.as_mut() else {
            return;
        };
        let Some(saved) = &draft.saved else { return };
        if tab.busy
            || draft.busy
            || draft.page != Page::Sql
            || !(saved.view || draft.preview_save_as)
            || !matches!(
                draft.sql_definition,
                SqlDefinition::Unloaded | SqlDefinition::Failed(_)
            )
        {
            return;
        }
        let command = Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::DesignSql {
                table: saved.definition.table.clone(),
            },
        };
        draft.sql_definition = SqlDefinition::Loading;
        draft.preview_revision = None;
        draft.preview_view.key = None;
        let generation = draft.sql_generation;
        self.submit(id, command, Purpose::DesignSql(generation));
    }
    pub(super) fn refresh_design_info(
        &mut self,
        id: &str,
        page: tomato_core::table_design::alter::InfoPage,
    ) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &tab.panel
        else {
            return;
        };
        let Some(draft) = &workbench.designer else {
            return;
        };
        if tab.busy || draft.busy {
            return;
        }
        let staged = match page {
            tomato_core::table_design::alter::InfoPage::Indexes => draft.index_changes(),
            tomato_core::table_design::alter::InfoPage::ForeignKeys => draft.info[1].changed(
                draft
                    .saved
                    .as_ref()
                    .map_or(&[], |s| s.foreign_keys.rows.as_slice()),
            ),
            tomato_core::table_design::alter::InfoPage::Triggers => draft.info[2].changed(
                draft
                    .saved
                    .as_ref()
                    .map_or(&[], |s| s.triggers.rows.as_slice()),
            ),
        };
        if staged {
            return;
        }
        let Some(saved) = &draft.saved else { return };
        let command = Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::DesignInfo {
                table: saved.definition.table.clone(),
                page,
            },
        };
        self.submit(id, command, Purpose::Normal);
    }
    pub(super) fn refresh_design_dependents(
        &mut self,
        owner: &str,
        connection: &Connection,
        table: &TableId,
    ) {
        if let Some(host) = self.sql_hosts.get_mut(&connection.id)
            && let Some(tables) = host.tables_for_mut(connection, &table.schema)
            && !tables.iter().any(|item| item.table == *table)
        {
            let values = std::sync::Arc::make_mut(tables);
            values.push(tomato_core::workbench::TableInfo {
                table: table.clone(),
                view: false,
            });
            values.sort_by(|a, b| a.table.name.cmp(&b.table.name));
        }
        let mut jobs = Vec::new();
        for tab in &mut self.tabs {
            if tab.id == owner {
                continue;
            }
            if let Panel::Sql {
                connection: other,
                workbench,
                ..
            } = &mut tab.panel
                && other.id == connection.id
                && !workbench.object_load_pending
                && tabs::workbench::session_database(other, &workbench.schema)
                    == tabs::workbench::session_database(connection, &table.schema)
            {
                if workbench.browser.is_some() {
                    if !tab.busy {
                        jobs.push((
                            tab.id.clone(),
                            Command::Database {
                                connection: other.clone(),
                                action: DatabaseAction::Objects {
                                    schema: workbench.schema.clone(),
                                },
                            },
                        ));
                    }
                } else if workbench.table.as_ref() == Some(table) && workbench.designer.is_none() {
                    if workbench.dirty() {
                        workbench.needs_refresh = true;
                        workbench.status = "表结构已修改，请先处理未保存的数据，再刷新".into();
                    } else if let Some(mut request) = workbench.request() {
                        request.sort = None;
                        request.offset = 0;
                        jobs.push((
                            tab.id.clone(),
                            Command::Database {
                                connection: other.clone(),
                                action: DatabaseAction::Page { request },
                            },
                        ));
                    }
                } else if workbench.table.is_none() && tab.source_id == connection.id {
                    jobs.push((
                        tab.id.clone(),
                        Command::Database {
                            connection: other.clone(),
                            action: DatabaseAction::Tables {
                                schema: table.schema.clone(),
                            },
                        },
                    ));
                } else if workbench.table.is_none() {
                    jobs.push((
                        tab.id.clone(),
                        Command::Database {
                            connection: other.clone(),
                            action: DatabaseAction::Completion {
                                schema: workbench.schema.clone(),
                            },
                        },
                    ));
                }
            }
        }
        for (id, job) in jobs {
            self.submit(&id, job, Purpose::Normal);
        }
    }
}
