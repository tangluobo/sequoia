use super::selection::{self, Group};
use crate::tabs::text_state::Editor;
use serde::{Deserialize, Serialize};
use tomato_core::{
    Connection,
    backup::{Scope, Selection},
};

#[derive(Default, Clone, Serialize, Deserialize)]
pub(crate) struct Options {
    pub comment: String,
    pub lock_tables: bool,
    pub single_transaction: bool,
    pub custom_filename: bool,
    pub filename: String,
}
pub(crate) struct Draft {
    pub id: String,
    pub connection: Connection,
    pub scope: Scope,
    pub namespace: Option<String>,
    pub options: Options,
    pub groups: [Group; 4],
    pub tab: usize,
    pub loading: bool,
    pub load_failed: bool,
    pub running: bool,
    pub error: Option<String>,
    pub complete: Option<String>,
    pub control: tomato_services::backup::Control,
    pub editors: [Editor; 3],
    pub started: Option<std::time::Instant>,
    pub elapsed_ms: Option<u64>,
    pub restore: Option<String>,
    pub choose_restore_source: bool,
    pub confirm_restore: bool,
    pub confirm_focus: bool,
    pub owner_rect: Option<egui::Rect>,
    pub notice: Option<String>,
    pub alert_focus: bool,
    pub log_text: String,
    pub template_groups: Option<[super::template::Group; 4]>,
    pub tree_scroll: f32,
    pub scrollbar: crate::native_scrollbar::State,
    pub tree_selected: Option<(tomato_core::backup::Kind, usize)>,
    pub scroll_areas: [Option<egui::Id>; 3],
}
impl Draft {
    pub fn new(connection: Connection, scope: Scope, ctx: &egui::Context) -> Self {
        let repaint = ctx.clone();
        Self {
            id: Connection::new("TOOL").id,
            connection,
            scope,
            namespace: None,
            options: Options::default(),
            groups: selection::groups(Vec::new()),
            tab: 0,
            loading: true,
            load_failed: false,
            running: false,
            error: None,
            complete: None,
            control: tomato_services::backup::Control::new(move || repaint.request_repaint()),
            editors: Default::default(),
            started: None,
            elapsed_ms: None,
            restore: None,
            choose_restore_source: false,
            confirm_restore: false,
            confirm_focus: false,
            owner_rect: None,
            notice: None,
            alert_focus: true,
            log_text: String::new(),
            template_groups: None,
            tree_scroll: 0.,
            scrollbar: Default::default(),
            tree_selected: None,
            scroll_areas: [None; 3],
        }
    }
    pub fn total(&self) -> usize {
        self.groups.iter().map(|group| group.items.len()).sum()
    }
    pub fn objects(&self) -> Vec<Selection> {
        self.groups
            .iter()
            .flat_map(Group::selected)
            .cloned()
            .collect()
    }
    pub fn release(&self, ctx: &egui::Context) {
        if self.running {
            self.control.cancel();
        }
        for editor in &self.editors {
            editor.release(ctx);
        }
        ctx.data_mut(|data| {
            for id in self.scroll_areas.into_iter().flatten() {
                data.remove::<egui::scroll_area::State>(id);
            }
        });
    }
}
