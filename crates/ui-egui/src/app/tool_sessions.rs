//! Tool views share one download actor per workspace and release it with the last view.
use super::*;
use std::sync::Arc;
mod fixture;
#[cfg(test)]
mod tests;
impl TomatoApp {
    pub(super) fn prepare_game_dungeons(&self, tool: &mut ToolState) {
        if let Some(state) = &mut tool.game_dungeons {
            state.path = self
                .repository
                .location()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(Store::default_path)
                .with_file_name(tomato_core::game_dungeons::FILE_NAME);
        }
    }
    pub(super) fn prepare_downloader(&mut self, tool: &mut ToolState) {
        if tool.kind != "downloader" {
            return;
        }
        let path = self
            .repository
            .location()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(Store::default_path)
            .with_file_name("downloads.json");
        let state = tool
            .downloader
            .get_or_insert_with(|| Box::new(crate::tabs::downloader::State::new(path.clone())));
        if state.manager.is_none() {
            state.path = path;
            let manager = self.download_manager.upgrade().unwrap_or_else(|| {
                let ctx = self.context.clone();
                Arc::new(tomato_services::downloader::Manager::open(
                    state.path.clone(),
                    move || ctx.request_repaint_of(egui::ViewportId::ROOT),
                ))
            });
            self.download_manager = Arc::downgrade(&manager);
            state.manager = Some(manager);
        }
    }
}
