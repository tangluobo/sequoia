use tomato_services::hosts::{Config, Snapshot, group_text, parse_entries};

#[derive(Default)]
pub struct State {
    pub paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
    pub editor: super::text_state::Editor,
    pub rename_editor: super::text_state::Editor,
    pub snapshot: Option<std::sync::Arc<Snapshot>>,
    pub selected: Option<usize>,
    pub text: String,
    pub pending_selection: Option<Option<usize>>,
    pub pending_rename: bool,
    pub pending_delete: bool,
    pub rename: Option<String>,
    pub delete: bool,
    pub status: String,
    pub status_until: Option<std::time::Instant>,
    pub failure: Option<String>,
    pub authorization: Option<AuthorizationPrompt>,
}

pub struct AuthorizationPrompt {
    pub request: Box<tomato_services::hosts::SaveRequest>,
    pub password: tomato_services::hosts::Password,
    pub control: tomato_services::backup::Control,
    pub editor: super::text_state::Editor,
    pub running: bool,
    pub cancel_requested: bool,
    pub error: Option<String>,
}
impl Drop for AuthorizationPrompt {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

impl State {
    pub fn fail(&mut self, ctx: &egui::Context, error: &anyhow::Error) {
        self.status.clear();
        self.status_until = None;
        if let Some(required) = error.downcast_ref::<tomato_services::hosts::ElevationRequired>() {
            let mut editor = super::text_state::Editor::default();
            editor.focus();
            self.authorization = Some(AuthorizationPrompt {
                request: required.0.clone(),
                password: Default::default(),
                control: Default::default(),
                editor,
                running: false,
                cancel_requested: false,
                error: None,
            });
        } else if self
            .authorization
            .as_ref()
            .is_some_and(|p| p.cancel_requested)
        {
            self.cancel_authorization(ctx);
        } else if let Some(prompt) = &mut self.authorization {
            prompt.running = false;
            prompt.error = Some(format!("{error:#}"));
            prompt.password = Default::default();
            prompt.editor.focus();
        } else {
            self.pending_selection = None;
            self.pending_rename = false;
            self.pending_delete = false;
            self.failure = Some(format!("{error:#}"));
        }
    }
    pub fn cancel_authorization(&mut self, ctx: &egui::Context) {
        if let Some(prompt) = self.authorization.take() {
            prompt.editor.release(ctx);
        }
        self.pending_selection = None;
        self.pending_rename = false;
        self.pending_delete = false;
        self.editor.focus();
    }
    pub fn candidate(&self) -> Config {
        let mut config = self
            .snapshot
            .as_ref()
            .map(|s| s.config.clone())
            .unwrap_or_default();
        if let Some(group) = self.selected.and_then(|i| config.groups.get_mut(i)) {
            let old = std::mem::take(&mut group.entries);
            group.entries = parse_entries(&self.text);
            let mut exact = std::collections::BTreeMap::<_, std::collections::VecDeque<_>>::new();
            for (index, entry) in old.iter().enumerate() {
                exact
                    .entry((entry.ip.as_str(), entry.domain.as_str()))
                    .or_default()
                    .push_back(index);
            }
            let mut matches: Vec<_> = group
                .entries
                .iter()
                .map(|entry| {
                    exact
                        .get_mut(&(entry.ip.as_str(), entry.domain.as_str()))
                        .and_then(|indices| indices.pop_front())
                })
                .collect();
            let used: std::collections::BTreeSet<_> = matches.iter().flatten().copied().collect();
            // Match unchanged rows first (including duplicates and reordering). If the row
            // count did not change, an edited row keeps its otherwise unclaimed metadata.
            if old.len() == group.entries.len() {
                let mut remaining = (0..old.len()).filter(|index| !used.contains(index));
                for found in &mut matches {
                    if found.is_none() {
                        *found = remaining.next();
                    }
                }
            }
            for (entry, found) in group.entries.iter_mut().zip(matches) {
                if let Some(index) = found {
                    entry.extra = old[index].extra.clone();
                }
            }
        }
        config
    }
    pub fn accept(&mut self, snapshot: Snapshot) {
        self.authorization = None;
        let initial = self.snapshot.is_none();
        if let Some(selected) = self.pending_selection.take() {
            self.selected = selected;
        }
        if self
            .selected
            .is_some_and(|i| i >= snapshot.config.groups.len())
        {
            self.selected = None;
        }
        self.editor.reset();
        self.text = self
            .selected
            .and_then(|i| snapshot.config.groups.get(i))
            .map(group_text)
            .unwrap_or_else(|| snapshot.system.clone());
        self.rename = None;
        self.rename_editor.reset();
        self.delete = false;
        if self.pending_rename
            && let Some(i) = self.selected
        {
            self.rename = snapshot.config.groups.get(i).map(|g| g.name.clone());
            self.rename_editor.focus();
        }
        if self.pending_delete {
            self.delete = true;
        }
        self.pending_rename = false;
        self.pending_delete = false;
        self.snapshot = Some(std::sync::Arc::new(snapshot));
        self.status = if initial {
            String::new()
        } else {
            "配置已保存".into()
        };
        self.status_until =
            (!initial).then(|| std::time::Instant::now() + std::time::Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_and_reordering_duplicate_entries_preserves_unknown_row_and_group_fields() {
        let config: Config = serde_json::from_value(serde_json::json!({"future":{"v":1},"groups":[{"name":"开发","enabled":false,"futureGroup":true,"entries":[{"ip":"1","domain":"a","future":1},{"ip":"2","domain":"b","future":2},{"ip":"1","domain":"a","future":3}]}]})).unwrap();
        let mut state = State {
            selected: Some(0),
            ..Default::default()
        };
        state.accept(Snapshot {
            config,
            system: String::new(),
            config_stamp: None,
            system_stamp: None,
        });
        state.text = "2 b\n1 a\n3 changed".into();
        let next = state.candidate();
        let entries = &next.groups[0].entries;
        assert_eq!(
            entries
                .iter()
                .map(|e| e.extra["future"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [2, 1, 3]
        );
        assert_eq!(next.extra["future"]["v"], 1);
        assert_eq!(next.groups[0].extra["futureGroup"], true);
    }
}
