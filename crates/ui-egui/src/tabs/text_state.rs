//! Ownership for egui's otherwise persistent text-edit undo snapshots.
pub struct Editor {
    id: Option<egui::Id>,
    reset: bool,
    focus: bool,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            id: None,
            reset: true,
            focus: false,
        }
    }
}
impl Editor {
    /// Request focus when this editor is next placed in its actual UI hierarchy.
    pub fn focus(&mut self) {
        self.focus = true;
    }
    pub fn apply_focus(&mut self, response: &egui::Response) {
        if std::mem::take(&mut self.focus) {
            response.request_focus();
        }
    }
    pub fn select(&self, ctx: &egui::Context, range: std::ops::Range<usize>) {
        if let Some(id) = self.id {
            let mut state =
                egui::widgets::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(range.start),
                    egui::text::CCursor::new(range.end),
                )));
            state.store(ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }
    pub fn replace(&mut self, ctx: &egui::Context, text: &mut String, value: String) -> bool {
        if *text == value {
            return false;
        }
        if let Some(id) = self.id {
            if std::mem::take(&mut self.reset) {
                ctx.data_mut(|data| data.remove::<egui::widgets::text_edit::TextEditState>(id));
            }
            let mut state =
                egui::widgets::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
            let range = state.cursor.char_range().unwrap_or_default();
            let mut undo = state.take_undoer();
            undo.add_undo_owned((range, text.clone()));
            *text = value;
            let range =
                egui::text::CCursorRange::one(egui::text::CCursor::new(text.chars().count()));
            state.cursor.set_char_range(Some(range));
            undo.add_undo_owned((range, text.clone()));
            state.set_undoer(undo);
            state.trim_undo_memory(32 * 1024 * 1024);
            state.store(ctx, id);
        } else {
            *text = value;
        }
        true
    }
    pub fn reset(&mut self) {
        self.reset = true;
    }
    pub fn prepare(&mut self, ui: &egui::Ui, name: &str) -> egui::Id {
        let id = ui.make_persistent_id(name);
        self.prepare_id(ui.ctx(), id)
    }
    pub fn prepare_id(&mut self, ctx: &egui::Context, id: egui::Id) -> egui::Id {
        self.id = Some(id);
        if std::mem::take(&mut self.reset) {
            ctx.data_mut(|data| data.remove::<egui::widgets::text_edit::TextEditState>(id));
        }
        id
    }
    pub fn trim(&self, ctx: &egui::Context) {
        if let Some(id) = self.id
            && let Some(state) = egui::widgets::text_edit::TextEditState::load(ctx, id)
        {
            state.trim_undo_memory(32 * 1024 * 1024);
            state.store(ctx, id);
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        if let Some(id) = self.id {
            ctx.data_mut(|data| data.remove::<egui::widgets::text_edit::TextEditState>(id));
        }
    }
}
