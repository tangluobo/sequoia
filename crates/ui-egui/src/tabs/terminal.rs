pub mod control;
pub mod cursor;
#[derive(Default)]
pub struct TerminalViewState {
    pub control: control::Control,
    pub cursor: cursor::CursorBlink,
    pub font_size: Option<f32>,
    pub composition: String,
    pub focused_once: bool,
    pub input_line: String,
    pub answers: Vec<String>,
    pub selection: Option<Selection>,
    pub selection_snapshot: Option<String>,
    pub selection_anchor: Option<Selection>,
    pub paused_output: bool,
    pub next_drag_scroll: f64,
    pub alternate: bool,
    pub ssh: Option<Box<super::ssh::Workspace>>,
    pub initial_command: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Selection {
    pub anchor: (u64, u16),
    pub end: (u64, u16),
}
impl TerminalViewState {
    pub fn deactivate(&mut self) {
        self.focused_once = false;
        self.composition.clear();
    }
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_anchor = None;
        self.selection_snapshot = None;
    }
    pub fn has_selection(&self) -> bool {
        self.selection.is_some() || self.selection_snapshot.is_some()
    }
}
