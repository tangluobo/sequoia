//! Original standalone file-browser tab actions share the live file workspace.
use super::*;
use tomato_services::terminal::ssh::tools::Section;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Refresh,
    Session,
}

pub(super) fn supports(tab: &Tab) -> bool {
    matches!(&tab.panel, Panel::Terminal { connection, view, .. }
        if matches!(connection.kind(), "SFTP" | "FTP" | "S3" | "ALIYUN_OSS")
            && view.ssh.as_ref().is_some_and(|workspace| workspace.standalone == Some(Section::Files)))
}

pub(super) fn menu(ui: &mut Ui, tab: &Tab) -> Option<Action> {
    if !supports(tab) {
        return None;
    }
    let mut action = None;
    if ui.add(crate::compact_menu::Item::new("刷新")).clicked() {
        action = Some(Action::Refresh);
    }
    ui.separator();
    if ui.add(crate::compact_menu::Item::new("会话配置")).clicked() {
        action = Some(Action::Session);
    }
    if action.is_some() {
        ui.close();
    }
    action
}

impl TomatoApp {
    pub(super) fn file_tab_action(&mut self, id: &str, action: Action) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id && supports(tab))
        else {
            return;
        };
        match action {
            Action::Session => self.open_session_config(id),
            Action::Refresh => {
                if let Panel::Terminal { view, .. } = &mut tab.panel
                    && let Some(workspace) = &mut view.ssh
                {
                    // The normal workspace dispatcher coalesces refreshes, preserves
                    // the current directory, and runs listing outside the UI frame.
                    workspace.panes.entry(Section::Files).or_default().refresh = true;
                    self.context.request_repaint();
                }
            }
        }
    }
}
