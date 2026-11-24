use std::{
    collections::{BTreeSet, VecDeque},
    time::Instant,
};
use tomato_core::server::{ServerAccount, ServerConfig, SharedDirectory};
pub struct ServerView {
    pub config: ServerConfig,
    pub port: String,
    pub running: Option<tomato_services::network::RunningServer>,
    pub page: usize,
    pub selected: Option<usize>,
    pub selection: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub dialog: Option<Edit>,
    pub logs: VecDeque<String>,
    pub changed: bool,
    pub save_failed: bool,
    pub last_edit: Instant,
}

pub enum Edit {
    Share {
        index: Option<usize>,
        value: SharedDirectory,
        users: String,
    },
    Account {
        index: Option<usize>,
        value: ServerAccount,
    },
    Remove {
        indices: Vec<usize>,
        account: bool,
    },
    ConfirmStart,
}
impl ServerView {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            port: config.port.to_string(),
            config,
            running: None,
            page: 0,
            selected: None,
            selection: Default::default(),
            anchor: None,
            dialog: None,
            logs: Default::default(),
            changed: false,
            save_failed: false,
            last_edit: Instant::now(),
        }
    }
    pub fn log(&mut self, text: impl AsRef<str>) {
        self.logs.push_back(format!(
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            text.as_ref()
        ));
        while self.logs.len() > 1000 {
            self.logs.pop_front();
        }
    }
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.anchor = None;
        self.selection.clear();
    }
    pub fn select(&mut self, index: usize, toggle: bool, range: bool) {
        if range {
            let anchor = self.anchor.unwrap_or(index);
            if !toggle {
                self.selection.clear();
            }
            self.selection.extend(anchor.min(index)..=anchor.max(index));
        } else {
            if toggle {
                if !self.selection.insert(index) {
                    self.selection.remove(&index);
                }
            } else {
                self.selection.clear();
                self.selection.insert(index);
            }
            self.anchor = Some(index);
        }
        self.selected = if self.selection.contains(&index) {
            Some(index)
        } else {
            self.selection.first().copied()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_selection_supports_control_shift_and_page_reset() {
        let mut view = ServerView::new(ServerConfig::default());
        view.select(2, false, false);
        view.select(4, true, false);
        assert_eq!(view.selection.iter().copied().collect::<Vec<_>>(), [2, 4]);
        view.select(6, false, true);
        assert_eq!(
            view.selection.iter().copied().collect::<Vec<_>>(),
            [4, 5, 6]
        );
        view.select(1, true, true);
        assert_eq!(
            view.selection.iter().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6]
        );
        view.select(1, true, false);
        assert!(!view.selection.contains(&1));
        assert_eq!(view.selected, Some(2));
        view.clear_selection();
        assert!(view.selection.is_empty());
        assert!(view.selected.is_none());
        assert!(view.anchor.is_none());
    }
}
