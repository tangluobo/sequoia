//! Window draft; deliberately separate from the tested object/selection model.
use super::State;
use tomato_core::database_objects::{Catalog, Object};
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Icon,
    Detail,
    Er,
}
pub struct Draft {
    pub state: State,
    pub generation: u64,
    pub header_pending: bool,
    pub fields_loading: FieldsLoading,
    pub mode: Mode,
    pub widths: [f32; 7],
    pub widths_pending: bool,
    pub detail_rows: Vec<usize>,
    pub sort: Option<(usize, bool)>,
    pub rename: Option<Rename>,
    pub confirmation: Option<Confirmation>,
    pub search_due: Option<f64>,
    pub alert_focus: bool,
    pub notice: Option<String>,
    pub search_editor: crate::tabs::text_state::Editor,
    pub rename_editor: crate::tabs::text_state::Editor,
    pub rename_due: Option<(usize, f64)>,
    pub drag_selection: Option<DragSelection>,
    pub(crate) bars: [crate::native_scrollbar::State; 2],
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum FieldsLoading {
    NotRequested,
    Pending,
    #[default]
    Ready,
    Failed,
}
#[derive(Clone, Copy)]
pub enum DragSelection {
    Marquee(egui::Vec2),
    Rows(usize),
}
pub struct Rename {
    pub index: usize,
    pub text: String,
    pub focus: bool,
}
#[derive(Clone, Copy)]
pub enum Mutation {
    Drop,
    Clear,
    Truncate,
}
pub struct Confirmation {
    pub mutation: Mutation,
    pub targets: Vec<tomato_core::database_objects::Target>,
}
pub enum Action {
    Open(usize),
    OpenSelected { structure: bool },
    NewTable,
    NewQuery,
    Import,
    Export,
    Refresh,
    Mutate(tomato_core::database_objects::Request),
    Rename(usize, String),
    Copy,
    Paste(Option<String>),
}
impl std::ops::Deref for Draft {
    type Target = State;
    fn deref(&self) -> &State {
        &self.state
    }
}
impl std::ops::DerefMut for Draft {
    fn deref_mut(&mut self) -> &mut State {
        &mut self.state
    }
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            state: State::default(),
            generation: 0,
            header_pending: false,
            fields_loading: FieldsLoading::Ready,
            mode: Mode::Icon,
            widths: [160., 100., 140., 100., 100., 70., 200.],
            widths_pending: true,
            detail_rows: Vec::new(),
            sort: None,
            rename: None,
            confirmation: None,
            search_due: None,
            alert_focus: true,
            notice: None,
            search_editor: Default::default(),
            rename_editor: Default::default(),
            rename_due: None,
            drag_selection: None,
            bars: Default::default(),
        }
    }
}
impl Draft {
    pub fn start_rename(&mut self, ctx: &egui::Context, index: usize) {
        self.rename_due = None;
        if self.selected.len() != 1 || !self.selected.contains(&index) {
            return;
        }
        let Some(name) = self
            .catalog
            .objects
            .get(index)
            .map(|object| object.table.name.clone())
        else {
            return;
        };
        self.rename_editor.release(ctx);
        self.rename_editor.reset();
        self.rename = Some(Rename {
            index,
            text: name,
            focus: true,
        });
        ctx.request_repaint();
    }
    pub fn confirm(&mut self, mutation: Mutation) {
        let targets = self
            .selection_order
            .iter()
            .filter_map(|index| self.catalog.objects.get(*index))
            .filter(|o| matches!(mutation, Mutation::Drop) || !o.view)
            .map(|o| tomato_core::database_objects::Target {
                table: o.table.clone(),
                view: o.view,
            })
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            self.confirmation = Some(Confirmation { mutation, targets });
            self.alert_focus = true;
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        self.search_editor.release(ctx);
        self.rename_editor.release(ctx);
    }
    pub fn receive_shared(&mut self, catalog: std::sync::Arc<Catalog>) {
        self.header_pending = false;
        self.fields_loading = FieldsLoading::Ready;
        self.state.fields_pending = false;
        self.rename_due = None;
        self.drag_selection = None;
        self.rename = None;
        self.state.selected.clear();
        self.state.selection_order.clear();
        self.state.anchor = None;
        self.state.primary = None;
        self.state.catalog = catalog;
        self.state.filter();
        self.refresh_detail();
    }
    pub fn begin_headers(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.header_pending = true;
        self.fields_loading = FieldsLoading::NotRequested;
        self.state.fields_pending = true;
    }
    pub fn receive_headers(&mut self, catalog: std::sync::Arc<Catalog>) {
        self.receive_shared(catalog);
        if !self.catalog.objects.is_empty() {
            self.fields_loading = FieldsLoading::NotRequested;
            self.state.fields_pending = true;
        }
    }
    pub fn fail_fields(&mut self) {
        self.fields_loading = FieldsLoading::Failed;
        self.state.fields_pending = false;
        if !self.search.trim_matches(|c: char| c <= ' ').is_empty() {
            self.filter();
        }
    }
    pub fn filter(&mut self) {
        self.rename_due = None;
        self.drag_selection = None;
        self.state.filter();
        self.refresh_detail();
    }
    fn refresh_detail(&mut self) {
        self.detail_rows.clear();
        self.detail_rows.extend_from_slice(&self.state.filtered);
        self.sort_detail();
        self.widths_pending = true;
    }
    fn sort_detail(&mut self) {
        if let Some((column, descending)) = self.sort {
            let catalog = &self.state.catalog;
            self.detail_rows.sort_by(|left, right| {
                let order = field(&catalog.objects[*left], column)
                    .encode_utf16()
                    .cmp(field(&catalog.objects[*right], column).encode_utf16());
                if descending { order.reverse() } else { order }
            });
        }
    }
    pub fn cycle_sort(&mut self, column: usize) {
        self.rename_due = None;
        self.sort = match self.sort {
            Some((old, false)) if old == column => Some((column, true)),
            Some((old, true)) if old == column => None,
            _ => Some((column, false)),
        };
        self.detail_rows.clear();
        self.detail_rows.extend_from_slice(&self.state.filtered);
        self.sort_detail();
    }
    pub fn click_detail(&mut self, index: usize, control: bool, shift: bool) {
        // TableView ranges use sorted visual rows; icon/ER order remains unchanged.
        std::mem::swap(&mut self.state.filtered, &mut self.detail_rows);
        self.state.click(index, control, shift);
        std::mem::swap(&mut self.state.filtered, &mut self.detail_rows);
        let selected = &self.state.selected;
        self.state.selection_order.clear();
        self.state.selection_order.extend(
            self.detail_rows
                .iter()
                .copied()
                .filter(|index| selected.contains(index)),
        );
        self.state.primary = self.state.selection_order.first().copied();
    }
}
pub fn field(object: &Object, column: usize) -> &str {
    match column {
        0 => &object.table.name,
        1 => &object.auto_increment,
        2 => &object.update_time,
        3 => &object.data_length,
        4 => &object.engine,
        5 => &object.rows,
        _ => &object.comment,
    }
}
