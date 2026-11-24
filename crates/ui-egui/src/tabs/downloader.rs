use std::path::PathBuf;
use tomato_services::downloader::{Manager, Record, Settings};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Add,
    Settings,
}
pub struct Dialog {
    pub kind: Kind,
    pub url: String,
    pub directory: String,
    pub threads: String,
    pub concurrent: String,
    pub error: Option<String>,
    pub focus: bool,
}
impl Dialog {
    pub fn new(kind: Kind, settings: &Settings) -> Self {
        Self {
            kind,
            url: String::new(),
            directory: settings.directory.clone(),
            threads: settings.threads.to_string(),
            concurrent: settings.concurrent.to_string(),
            error: None,
            focus: true,
        }
    }
}
pub struct State {
    pub show_header: bool,
    pub loaded_settings: bool,
    pub manager: Option<std::sync::Arc<Manager>>,
    pub path: PathBuf,
    pub revision: u64,
    pub rows: Vec<Record>,
    pub error: Option<String>,
    pub last_error: Option<String>,
    pub error_focus: bool,
    pub dialog: Option<Box<Dialog>>,
    pub defaults: Settings,
    pub selected: Option<String>,
    pub columns: [f32; 8],
    pub rates: std::collections::HashMap<String, (u64, std::time::Instant, f64)>,
}
impl State {
    pub fn new(path: PathBuf) -> Self {
        Self {
            show_header: true,
            loaded_settings: false,
            manager: None,
            path,
            revision: u64::MAX,
            rows: vec![],
            error: None,
            last_error: None,
            error_focus: true,
            dialog: None,
            defaults: Settings::default(),
            selected: None,
            columns: [235., 90., 190., 105., 90., 105., 120., 80.],
            rates: Default::default(),
        }
    }
    pub fn open(&mut self, kind: Kind) {
        self.dialog = Some(Box::new(Dialog::new(kind, &self.defaults)));
    }
    pub fn send(&mut self, action: tomato_services::downloader::Action) {
        if let Some(manager) = &self.manager
            && let Err(error) = manager.send(action)
        {
            self.error = Some(error.to_string());
            self.error_focus = true;
        }
    }
}
