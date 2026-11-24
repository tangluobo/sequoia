//! A connection cannot disappear under unsaved inner editors.
use super::*;
impl TomatoApp {
    pub(in crate::app) fn close_file_children(&mut self, owner: &str) -> bool {
        let children:Vec<_>=self.tabs.iter().filter(|t|matches!(&t.panel,Panel::Editor{markdown,..}if markdown.file_embedded&&markdown.file_owner.as_deref()==Some(owner))).map(|t|t.id.clone()).collect();
        if let Some(id) = children
            .iter()
            .find(|id| self.tabs.iter().any(|t| &t.id == *id && t.dirty()))
        {
            if self.confirmation.is_none() {
                self.file_owner_close = Some(owner.into());
                self.select_file_document(owner, id, true);
                self.confirmation = Some(Confirmation::Close(id.clone()));
            }
            return false;
        }
        for child in children {
            self.close_tab(&child, false);
        }
        true
    }
    pub(in crate::app) fn continue_file_owner_close(&mut self) {
        if self.confirmation.is_some() {
            return;
        }
        let Some(owner) = self.file_owner_close.clone() else {
            return;
        };
        if self.tabs.iter().any(|t|matches!(&t.panel,Panel::Editor{markdown,..}if markdown.file_embedded&&markdown.file_owner.as_deref()==Some(&owner))&&(t.busy||t.error.is_some())){return;}
        self.file_owner_close = None;
        self.close(&owner);
    }
    pub(in crate::app) fn closing_file_document(&mut self, id: &str) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let owner = match &self.tabs[index].panel {
            Panel::Editor { markdown, .. } if markdown.file_embedded => markdown.file_owner.clone(),
            _ => None,
        };
        if let Some(owner) = owner {
            let replacement=self.tabs[..index].iter().rev().find(|t|matches!(&t.panel,Panel::Editor{markdown,..}if markdown.file_embedded&&markdown.file_owner.as_deref()==Some(&owner))).map(|t|t.id.clone());
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == owner)
                && let Panel::Terminal { view, .. } = &mut tab.panel
                && let Some(workspace) = &mut view.ssh
                && workspace.document_active.as_deref() == Some(id)
            {
                workspace.document_active = replacement;
            }
        }
    }
}
