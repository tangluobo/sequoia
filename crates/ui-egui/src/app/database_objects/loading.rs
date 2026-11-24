//! Header requests are foreground work. Field preload is quiet and belongs to
//! exactly one tab generation and physical database/namespace snapshot.
use super::*;
use crate::tabs::database_objects::window::FieldsLoading;
#[cfg(test)]
mod tests;

pub(in crate::app) struct Stamp {
    generation: u64,
    connection: String,
    database: String,
    schema: String,
}
impl Stamp {
    fn new(connection: &Connection, schema: &str, generation: u64) -> Self {
        Self {
            generation,
            connection: connection.id.clone(),
            database: tabs::workbench::session_database(connection, schema).into(),
            schema: schema.into(),
        }
    }
    fn matches(&self, connection: &Connection, workbench: &tabs::workbench::Workbench) -> bool {
        self.connection == connection.id
            && self.schema == workbench.schema
            && self.database == tabs::workbench::session_database(connection, &workbench.schema)
            && workbench
                .browser
                .as_ref()
                .is_some_and(|b| b.generation == self.generation)
    }
}
impl TomatoApp {
    pub(in crate::app) fn submit_object_read(&mut self, id: &str, command: &Command) -> bool {
        let Command::Database { connection, action } = command else {
            return false;
        };
        let (schema, catalog) = match action {
            DatabaseAction::Objects { schema } | DatabaseAction::ObjectHeaders { schema } => {
                (schema, None)
            }
            DatabaseAction::ObjectFields { schema, catalog } => (schema, Some(catalog)),
            _ => return false,
        };
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        let Panel::Sql {
            connection: actual,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return false;
        };
        let Some(browser) = &mut workbench.browser else {
            return false;
        };
        if schema != &workbench.schema
            || connection.id != actual.id
            || tabs::workbench::session_database(connection, schema)
                != tabs::workbench::session_database(actual, &workbench.schema)
        {
            return true;
        }
        let fields = catalog.is_some();
        let action = if let Some(catalog) = catalog {
            if browser.header_pending
                || browser.fields_loading != FieldsLoading::NotRequested
                || !std::sync::Arc::ptr_eq(catalog, &browser.catalog)
            {
                return true;
            }
            DatabaseAction::ObjectFields {
                schema: schema.clone(),
                catalog: catalog.clone(),
            }
        } else {
            browser.begin_headers();
            DatabaseAction::ObjectHeaders {
                schema: schema.clone(),
            }
        };
        let stamp = Stamp::new(connection, schema, browser.generation);
        let purpose = if fields {
            Purpose::ObjectFields(stamp)
        } else {
            Purpose::ObjectHeaders(stamp)
        };
        let command = Command::Database {
            connection: connection.clone(),
            action,
        };
        if fields && !self.queue.can_submit(&command) {
            // Keep NotRequested; the next actual task completion retries it.
            return true;
        }
        if fields {
            browser.fields_loading = FieldsLoading::Pending;
        }
        match self.queue.submit(command) {
            Ok(task) => {
                self.pending.retain(|_, pending| {
                    pending.tab_id != id
                        || if fields {
                            !matches!(pending.purpose, Purpose::ObjectFields(_))
                        } else {
                            !matches!(
                                pending.purpose,
                                Purpose::ObjectFields(_) | Purpose::ObjectHeaders(_)
                            )
                        }
                });
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: id.into(),
                        purpose,
                    },
                );
                if !fields {
                    tab.busy = true;
                    tab.error = None;
                    workbench.execution = None;
                }
            }
            Err(error) => {
                browser.header_pending = false;
                browser.fail_fields();
                if !fields {
                    tab.busy = false;
                    browser.notice = Some(format!("操作未提交: {error}"));
                }
            }
        }
        true
    }
    pub(in crate::app) fn load_browser_fields(&mut self, id: &str) {
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
        let Some(browser) = &workbench.browser else {
            return;
        };
        if browser.header_pending
            || browser.fields_loading != FieldsLoading::NotRequested
            || browser.catalog.objects.is_empty()
        {
            return;
        }
        let command = Command::Database {
            connection: connection.clone(),
            action: DatabaseAction::ObjectFields {
                schema: workbench.schema.clone(),
                catalog: browser.catalog.clone(),
            },
        };
        self.submit_object_read(id, &command);
    }
    pub(in crate::app) fn accept_browser_header(&mut self, id: &str, stamp: &Stamp) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return false;
        };
        if !stamp.matches(connection, workbench) {
            return false;
        }
        let browser = workbench.browser.as_mut().expect("validated browser");
        if !browser.header_pending {
            return false;
        }
        browser.header_pending = false;
        browser.fields_loading = FieldsLoading::Failed;
        browser.state.fields_pending = false;
        true
    }
    pub(in crate::app) fn browser_fields_reply(
        &mut self,
        id: &str,
        stamp: &Stamp,
        result: anyhow::Result<Output>,
    ) {
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
        if !stamp.matches(connection, workbench)
            || workbench
                .browser
                .as_ref()
                .is_none_or(|b| b.fields_loading != FieldsLoading::Pending)
        {
            return;
        }
        if let Ok(Output::Database(DatabaseOutput::ObjectFields { schema, fields })) = result
            && schema == workbench.schema
            && workbench.receive_object_fields(fields).is_ok()
        {
            return;
        }
        if let Some(browser) = &mut workbench.browser {
            browser.fail_fields();
        }
    }
}
