use std::sync::Arc;
use tomato_core::sql_completion::{Completion, Metadata};
#[derive(Default)]
pub struct State {
    pub focus_requested: bool,
    pub changed: bool,
    pub save_requested: bool,
    pub editor: super::text_state::Editor,
    pub metadata: Option<Arc<Metadata>>,
    pub generation: u64,
    pub error: String,
    pub galley: Option<Arc<egui::Galley>>,
    pub galley_pos: egui::Pos2,
    pub text_rect: Option<egui::Rect>,
    pub copy_drag: Option<CopyDrag>,
    pub menu_event: Option<egui::Event>,
    pub caret: usize,
    pub requested: bool,
    pub catalog_changed: bool,
    pub matches: Completion,
    pub selected: usize,
    pub open: bool,
    pub consumed_enter: bool,
    pub selection: egui::text::CCursorRange,
    pub run_sql: Option<String>,
    pub ai_requested: Option<bool>,
    pub run_enabled: bool,
    pub popup_rect: Option<egui::Rect>,
    pub reveal: bool,
    pub pressed: Option<usize>,
}

pub struct CopyDrag {
    pub source: egui::text::CCursorRange,
    pub origin: egui::Pos2,
    pub frame: u64,
    pub preview: Option<Vec<String>>,
    pub cancelled: bool,
}

impl State {
    pub fn catalog(&self) -> &Metadata {
        static EMPTY: std::sync::LazyLock<Metadata> = std::sync::LazyLock::new(Metadata::default);
        self.metadata.as_deref().unwrap_or(&EMPTY)
    }
    pub fn invalidate(&mut self) {
        self.galley = None;
        self.copy_drag = None;
        self.dismiss();
    }
    pub fn dismiss(&mut self) {
        self.requested = false;
        self.open = false;
        self.popup_rect = None;
        self.pressed = None;
        self.matches = Completion::default();
    }
}
