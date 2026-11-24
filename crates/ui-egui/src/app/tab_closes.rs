//! Source close-left/right/all: snapshot order, skip pinned object tabs, honor each close request.
use super::*;
#[derive(Clone, Copy)]
pub(super) enum Side {
    Left,
    Right,
    All,
}
fn visible(tab: &Tab) -> bool {
    tab.workspace_visible()
}
fn closable(tab: &Tab) -> bool {
    visible(tab) && !matches!(&tab.panel,Panel::Sql{workbench,..}if workbench.browser.is_some())
}
fn awaiting_save(tab: &Tab) -> bool {
    match &tab.panel {
        Panel::Editor { markdown, .. } => markdown.close_after_save,
        Panel::Sql { workbench, .. } => workbench
            .document
            .as_ref()
            .is_some_and(|document| document.close_after_save),
        _ => false,
    }
}
impl TomatoApp {
    pub(super) fn close_tab_group(&mut self, anchor: &str, side: Side) {
        // Do not replace a batch waiting for a save or confirmation.
        if !self.closing_tabs.is_empty()
            || self.confirmation.is_some()
            || self.tabs.iter().any(awaiting_save)
        {
            return;
        }
        let visible = self.tabs.iter().filter(|t| visible(t)).collect::<Vec<_>>();
        let Some(index) = visible.iter().position(|t| t.id == anchor) else {
            return;
        };
        self.closing_tabs = visible
            .into_iter()
            .enumerate()
            .filter(|(i, t)| {
                closable(t)
                    && match side {
                        Side::Left => *i < index,
                        Side::Right => *i > index,
                        Side::All => true,
                    }
            })
            .map(|(_, t)| t.id.clone())
            .collect();
        self.continue_tab_closes();
    }
    pub(super) fn continue_tab_closes(&mut self) {
        if self.confirmation.is_some()
            || self.error.is_some()
            || self.tabs.iter().any(awaiting_save)
        {
            return;
        }
        while let Some(id) = self.closing_tabs.pop_front() {
            let Some(tab) = self.tabs.iter().find(|t| t.id == id && closable(t)) else {
                continue;
            };
            if tab.dirty() {
                self.confirmation = Some(Confirmation::Close(id));
                return;
            }
            self.close(&id);
            if self.tabs.iter().any(|tab| tab.id == id) {
                self.closing_tabs.push_front(id);
                return;
            }
        }
    }
}
#[cfg(test)]
mod tests;
