//! View controller: drafts and commands, without filesystem I/O.
use std::path::PathBuf;
use tomato_core::game_dungeons::{Action, Board, Weekday};
use tomato_services::{commands::Command, game_dungeons::Snapshot};

pub struct State {
    pub path: PathBuf,
    pub snapshot: Option<Snapshot>,
    pub load_requested: bool,
    pub query: String,
    pub account_filter: Option<String>,
    pub editor: Option<Editor>,
    close_on_success: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            path: tomato_core::Store::default_path()
                .with_file_name(tomato_core::game_dungeons::FILE_NAME),
            snapshot: None,
            load_requested: false,
            query: String::new(),
            account_filter: None,
            editor: None,
            close_on_success: false,
        }
    }
}
pub enum Editor {
    Account {
        id: Option<String>,
        name: String,
    },
    Character {
        id: Option<String>,
        account_id: String,
        name: String,
    },
    Dungeon {
        id: Option<String>,
        name: String,
        reset_day: Weekday,
    },
    Manage,
    Delete {
        action: Action,
        description: String,
    },
}
impl State {
    pub fn board(&self) -> Option<&Board> {
        self.snapshot.as_ref().map(|s| &s.board)
    }
    pub fn accept(&mut self, snapshot: Snapshot) {
        if self.close_on_success {
            self.editor = None;
        }
        self.close_on_success = false;
        self.snapshot = Some(snapshot);
        self.load_requested = true;
        if self
            .account_filter
            .as_ref()
            .is_some_and(|id| !self.board().unwrap().accounts.iter().any(|a| &a.id == id))
        {
            self.account_filter = None;
        }
    }
    pub fn command(&mut self, action: Action) -> Command {
        self.load_requested = true;
        self.close_on_success = !matches!(action, Action::Load | Action::SetCompleted { .. });
        Command::GameDungeons {
            path: self.path.clone(),
            action,
            revision: self.snapshot.as_ref().and_then(|s| s.revision.clone()),
        }
    }
    pub fn new_account(&mut self) {
        self.editor = Some(Editor::Account {
            id: None,
            name: String::new(),
        });
    }
    pub fn new_character(&mut self) {
        let account_id = self
            .account_filter
            .clone()
            .or_else(|| self.board()?.accounts.first().map(|a| a.id.clone()))
            .unwrap_or_default();
        self.editor = Some(Editor::Character {
            id: None,
            account_id,
            name: String::new(),
        });
    }
    pub fn new_dungeon(&mut self) {
        self.editor = Some(Editor::Dungeon {
            id: None,
            name: String::new(),
            reset_day: Weekday::default(),
        });
    }
}
