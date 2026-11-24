//! One editable ComboBox state, shared by all visible type cells in a draft.
pub struct Picker {
    query: String,
    pub focused: Option<usize>,
    pub open: bool,
    pub scroll: bool,
    pub popup: Option<egui::Id>,
    pub label_width: Option<f32>,
}
impl Picker {
    pub fn new(types: &[&str], current: &str) -> Self {
        Self {
            query: String::new(),
            focused: types
                .iter()
                .position(|value| *value == current)
                .or_else(|| (!types.is_empty()).then_some(0)),
            open: true,
            scroll: true,
            popup: None,
            label_width: None,
        }
    }
    pub fn matches(&self, value: &str) -> bool {
        self.query.is_empty()
            || value
                .as_bytes()
                .windows(self.query.len())
                .any(|part| part.eq_ignore_ascii_case(self.query.as_bytes()))
    }
    pub fn filter(&mut self, types: &[&str], text: &str) {
        text.trim().clone_into(&mut self.query);
        self.focused = self
            .focused
            .filter(|index| types.get(*index).is_some_and(|value| self.matches(value)))
            .or_else(|| types.iter().position(|value| self.matches(value)));
        self.open = true;
        self.scroll = true;
    }
    pub fn step(&mut self, types: &[&str], down: bool) {
        let mut visible = types
            .iter()
            .enumerate()
            .filter(|(_, value)| self.matches(value));
        self.focused = if down {
            visible
                .find(|(index, _)| self.focused.is_none_or(|current| *index > current))
                .map(|(index, _)| index)
                .or(self.focused)
        } else {
            visible
                .rfind(|(index, _)| self.focused.is_none_or(|current| *index < current))
                .map(|(index, _)| index)
                .or(self.focused)
        };
        self.scroll = true;
    }
    pub fn release(&self, ctx: &egui::Context) {
        if let Some(id) = self.popup {
            egui::Popup::close_id(ctx, id);
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
    }
}
