//! UI-only projection: retain row identities, never duplicate the page's cell payloads.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[derive(Default)]
pub struct Projection {
    pub order: Vec<usize>,
    pub hidden: BTreeSet<usize>,
    pub sorts: Vec<(usize, bool)>,
    pub filters: BTreeMap<usize, tomato_core::result_filter::Condition>,
    pub dialog_context: Option<(egui::Context, egui::Id)>,
    pub dialog: Option<crate::tabs::query_results::view::Dialog>,
    pub pending_write: Option<(PathBuf, String)>,
}

impl Drop for Projection {
    fn drop(&mut self) {
        if let Some((ctx, id)) = &self.dialog_context {
            if let Some(dialog) = &self.dialog {
                dialog.editor.release(ctx);
            }
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of((*id, "custom-filter-window")),
                egui::ViewportCommand::Close,
            );
        }
    }
}
