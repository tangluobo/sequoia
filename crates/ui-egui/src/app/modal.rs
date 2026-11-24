//! A panel-owned dialog can block the root only while its owner is rendered.
use super::*;

impl TomatoApp {
    pub(super) fn root_modal_open(&self) -> bool {
        self.instance_properties.is_some()
            || self.markdown_print.is_some()
            || self.markdown_html.as_ref().is_some_and(markdown_html::State::modal)
            ||self.credential.is_some()
            ||self.ssh_hosts.values().any(|h|h.job.as_ref().is_some_and(|j|j.auth.is_some()))
            || self.import_dialog.is_some()
            || self.import_reader.is_some()
            || self.export_dialog.is_some()
            || self.ai_dialog.is_some()
            || self.backup_dialog.is_some()
            || self.data_export.is_some()
            || self.data_import.is_some()
            || self.table_transfer.is_some()
            || self.database_admin.is_some()
            || self.session_config.is_some()
            || self.table_config.is_some()
            || self.tree_objects.is_some()
            || self.backup_edit.is_some()
            || self.mongo_dialog.is_some()
            || self.queries.dialog.is_some()
            || (self.section == 3
                && self
                    .development
                    .as_ref()
                    .is_some_and(|state| state.modal_open()))
            || self.visible_vnc_password()
            || self.visible_panel_modal_open()
            // These dialogs are rendered for every tab by query_result_dialogs,
            // independently of the active module/panel.
            || self.tabs.iter().any(|tab| {
                matches!(&tab.panel, Panel::Sql { workbench, .. }
                    if workbench.query_results.iter().any(|state| state.view.dialog.is_some()))
            })
    }

    pub(super) fn visible_vnc_password(&self) -> bool {
        self.section == 0
            && self.tabs.iter().any(|tab| {
                Some(&tab.id) == self.active.as_ref()
                    && matches!(
                        &tab.panel,
                        Panel::NativeDesktop { connection, session: Some(session), .. }
                        if connection.kind() == "VNC"
                            && session.status() == tomato_services::remote_desktop::Status::PasswordRequired
                    )
            })
    }

    fn visible_panel_modal_open(&self) -> bool {
        if self.section > 2 {
            return false;
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return false;
        };
        match &tab.panel {
            Panel::Tool(tool) => {
                tool.hosts.as_ref().is_some_and(|state| {
                    state.rename.is_some()
                        || state.delete
                        || state.failure.is_some()
                        || state.authorization.is_some()
                }) || tool
                    .downloader
                    .as_ref()
                    .is_some_and(|state| state.dialog.is_some() || state.error.is_some())
                    || tool.port_mapping.as_ref().is_some_and(|state| {
                        state.editor.is_some()
                            || state.delete.is_some()
                            || !state.reverse.is_empty()
                    })
            }
            Panel::Cloud { view, .. } => {
                view.record.is_some() || view.delete.is_some() || view.certificate.is_some()
            }
            Panel::MessageQueue { view, .. } => {
                view.create || view.confirm.is_some() || view.batch_notice.is_some()
            }
            Panel::Redis { view, .. } => view.dialog.is_some() || view.failure.is_some(),
            Panel::Terminal { view, .. } => view.ssh.as_ref().is_some_and(|w| {
                w.inspector_notice.is_some() || w.dialog.is_some() || w.storage.notice.is_some()
            }),
            Panel::Sql { workbench, .. } => {
                // Match the same browser-before-designer routing as sql::render.
                if let Some(browser) = &workbench.browser {
                    browser.confirmation.is_some() || browser.notice.is_some()
                } else {
                    workbench
                        .designer
                        .as_ref()
                        .is_some_and(|draft| draft.name.is_some() || draft.notice.is_some())
                        || workbench.failure.is_some()
                }
            }
            _ => false,
        }
    }
}
