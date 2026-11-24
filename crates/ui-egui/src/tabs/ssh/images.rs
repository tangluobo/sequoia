use super::*;
use std::{path::PathBuf, sync::Arc};
use tomato_services::files::images::{self, Asset, Mode, Request};
#[cfg(test)]
mod tests;
struct Thumbnail {
    request: Option<Request>,
    texture: Option<egui::TextureHandle>,
    used: u64,
}
impl Drop for Thumbnail {
    fn drop(&mut self) {
        if let Some(request) = &self.request {
            request.cancel.cancel();
        }
    }
}
#[derive(Clone)]
pub struct Item {
    pub name: String,
    pub path: String,
    pub size: u64,
}
#[derive(Default)]
pub struct Thumbnails {
    entries: std::collections::HashMap<String, Thumbnail>,
    pub queued: Vec<Request>,
    clock: u64,
    suspended: bool,
    gallery: Option<Arc<Vec<Item>>>,
    enabled: bool,
}
impl Thumbnails {
    pub(super) fn invalidate_gallery(&mut self) {
        self.gallery = None;
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.queued.clear();
        self.gallery = None;
        self.suspended = false;
    }
    pub fn suspend(&mut self) {
        self.clear();
        self.suspended = true;
    }
    pub fn set_mode(&mut self, enabled: bool) {
        if self.enabled && !enabled {
            self.entries.clear();
            self.queued.clear();
        }
        self.enabled = enabled;
    }
    pub fn visible(&mut self, entry: &Entry) -> Option<egui::TextureHandle> {
        if !self.enabled || self.suspended || entry.directory || !images::supported(&entry.name) {
            return None;
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(cached) = self.entries.get_mut(&entry.path) {
            cached.used = self.clock;
            return cached.texture.clone();
        }
        if self
            .entries
            .values()
            .filter(|entry| entry.request.is_some())
            .count()
            >= 4
        {
            return None;
        }
        if self.entries.len() >= 256
            && let Some(path) = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.request.is_none())
                .min_by_key(|(_, entry)| entry.used)
                .map(|(path, _)| path.clone())
        {
            self.entries.remove(&path);
        }
        let request = Request::new(entry.path.clone(), Mode::Thumbnail);
        self.queued.push(request.clone());
        self.entries.insert(
            entry.path.clone(),
            Thumbnail {
                request: Some(request),
                texture: None,
                used: self.clock,
            },
        );
        None
    }
    pub fn finish(
        &mut self,
        request: &Request,
        result: Result<Output, String>,
        ctx: &egui::Context,
    ) {
        let Some(entry) = self.entries.get_mut(&request.path).filter(|entry| {
            entry
                .request
                .as_ref()
                .is_some_and(|pending| pending.id == request.id)
        }) else {
            return;
        };
        entry.request = None;
        if !request.cancel.is_cancelled()
            && let Ok(Output::Image { asset, .. }) = result
        {
            entry.texture = Some(texture(ctx, &request.id.to_string(), asset.picture));
        }
    }
    pub fn reject(&mut self, request: &Request) {
        self.entries.remove(&request.path);
        request.cancel.cancel();
    }
    pub fn gallery(&mut self, entries: &[Entry]) -> Arc<Vec<Item>> {
        self.gallery
            .get_or_insert_with(|| {
                Arc::new(
                    entries
                        .iter()
                        .filter(|entry| !entry.directory && images::supported(&entry.name))
                        .map(|entry| Item {
                            name: entry.name.clone(),
                            path: entry.path.clone(),
                            size: entry.size,
                        })
                        .collect(),
                )
            })
            .clone()
    }
}
pub fn texture(
    ctx: &egui::Context,
    id: &str,
    picture: tomato_services::markdown::Picture,
) -> egui::TextureHandle {
    ctx.load_texture(
        format!("file-image:{id}"),
        egui::ColorImage::from_rgba_unmultiplied(
            [picture.width as usize, picture.height as usize],
            &picture.rgba,
        ),
        egui::TextureOptions::LINEAR,
    )
}
pub fn supported_connection(connection: &tomato_core::Connection) -> bool {
    matches!(connection.kind(), "SSH" | "SFTP" | "LOCAL_DIRECTORY")
        || tomato_services::files::object_copy::is_object(connection)
}
pub struct Preview {
    pub id: String,
    pub items: Arc<Vec<Item>>,
    pub index: usize,
    pub request: Request,
    pub queued: bool,
    pub texture: Option<egui::TextureHandle>,
    pub dimensions: egui::Vec2,
    pub title: String,
    pub error: String,
    pub closed: bool,
    pub scale: f32,
    pub initial_fit: bool,
    pub translation: egui::Vec2,
    pub inner_size: Option<egui::Vec2>,
    pub positioned: bool,
    pub animation: Option<images::Animation>,
    pub confirm_delete: bool,
    pub action: Option<PreviewAction>,
    pub notice: Option<(String, String)>,
}
#[derive(Clone)]
pub enum PreviewAction {
    Download(PathBuf),
    Delete,
}
impl Preview {
    pub fn new(items: Arc<Vec<Item>>, index: usize) -> Self {
        let request = Request::new(items[index].path.clone(), Mode::Full);
        Self {
            id: request.id.to_string(),
            items,
            index,
            request,
            queued: false,
            texture: None,
            dimensions: egui::Vec2::ZERO,
            title: "图片预览".into(),
            error: String::new(),
            closed: false,
            scale: 1.,
            initial_fit: true,
            translation: egui::Vec2::ZERO,
            inner_size: None,
            positioned: false,
            animation: None,
            confirm_delete: false,
            action: None,
            notice: None,
        }
    }
    pub fn select(&mut self, index: usize) {
        self.request.cancel.cancel();
        self.index = index;
        self.request = Request::new(self.items[index].path.clone(), Mode::Full);
        self.queued = false;
        self.texture = None;
        self.error.clear();
        self.initial_fit = true;
        self.translation = egui::Vec2::ZERO;
        self.animation = None;
    }
    pub fn accept(&mut self, asset: Asset, ctx: &egui::Context) {
        self.dimensions = egui::vec2(asset.width as f32, asset.height as f32);
        self.title = format!(
            "{}  |  {}x{}  |  {}  ({}/{})",
            self.items[self.index].name,
            asset.width,
            asset.height,
            display_size(self.items[self.index].size),
            self.index + 1,
            self.items.len()
        );
        self.texture = Some(texture(ctx, &self.id, asset.picture));
        if let Some(bytes) = asset.gif {
            let repaint = ctx.clone();
            let viewport = egui::ViewportId::from_hash_of(("file-image", &self.id));
            match images::Animation::start(bytes, move || repaint.request_repaint_of(viewport)) {
                Ok(animation) => self.animation = Some(animation),
                Err(error) => self.error = format!("图片加载失败: {error:#}"),
            }
        }
    }
    pub fn zoom(&mut self, delta: f32) {
        let next = self.scale * if delta < 0. { 1. / 1.05 } else { 1.05 };
        let size = self.dimensions * next;
        let max = self.dimensions.x * 10.;
        if size.x >= 50. && size.y >= 50. && size.x <= max && size.y <= max {
            self.scale = next;
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        self.request.cancel.cancel();
    }
}
fn display_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = "KB";
    for next in ["MB", "GB", "TB"] {
        if value < 1024. {
            break;
        }
        value /= 1024.;
        unit = next;
    }
    format!("{value:.1} {unit}")
}
impl Workspace {
    pub fn cancel_images(&mut self) {
        for preview in &mut self.previews {
            preview.request.cancel.cancel();
            preview.animation = None;
            preview.closed = true;
        }
        for pane in self.panes.values_mut() {
            pane.thumbnails.suspend();
        }
    }
    pub fn poll_image_actions(&mut self, session: &mut Terminal) {
        let actions = self
            .previews
            .iter_mut()
            .filter_map(|preview| {
                preview.action.take().map(|action| {
                    (
                        preview.id.clone(),
                        preview.items[preview.index].clone(),
                        action,
                    )
                })
            })
            .collect::<Vec<_>>();
        for (id, item, action) in actions {
            if self.pending.len() >= 4 {
                if let Some(preview) = self.previews.iter_mut().find(|preview| preview.id == id) {
                    preview.action = Some(action);
                }
                continue;
            }
            let operation = match &action {
                PreviewAction::Download(local) => Operation::Download {
                    remote: item.path.clone(),
                    local: local.clone(),
                },
                PreviewAction::Delete => Operation::Remove {
                    path: item.path.clone(),
                    directory: false,
                },
            };
            match session.tool_request(operation) {
                Ok(request) => {
                    self.pending.insert(
                        request,
                        Pending {
                            section: Section::Files,
                            mutation: true,
                            save: None,
                            copying: false,
                            uploading: false,
                            image: None,
                            export: None,
                            preview_action: Some((id, item, action)),
                            renamed: None,
                            change: None,
                        },
                    );
                }
                Err(error) => self.preview_action_reply((id, item, action), Err(error.to_string())),
            }
        }
    }
    pub fn preview_action_reply(
        &mut self,
        (id, item, action): (String, Item, PreviewAction),
        result: Result<Output, String>,
    ) {
        let Some(preview) = self.previews.iter_mut().find(|preview| preview.id == id) else {
            return;
        };
        match (action, result) {
            (PreviewAction::Download(path), Ok(_)) => {
                preview.notice = Some((
                    "下载完成".into(),
                    format!("文件已保存到: {}", path.display()),
                ))
            }
            (PreviewAction::Download(_), Err(error)) => {
                preview.notice = Some(("下载失败".into(), error))
            }
            (PreviewAction::Delete, Err(error)) => {
                preview.notice = Some(("删除失败".into(), error))
            }
            (PreviewAction::Delete, Ok(_)) => {
                let current = preview.items[preview.index].path.clone();
                Arc::make_mut(&mut preview.items).retain(|entry| entry.path != item.path);
                if preview.items.is_empty() {
                    preview.closed = true;
                } else {
                    let index = preview
                        .items
                        .iter()
                        .position(|entry| entry.path == current)
                        .unwrap_or_else(|| preview.index.min(preview.items.len() - 1));
                    preview.select(index);
                }
                let pane = self.panes.entry(Section::Files).or_default();
                pane.thumbnails.suspend();
                pane.refresh = true;
            }
        }
    }
    pub fn image_reply(
        &mut self,
        request: Request,
        result: Result<Output, String>,
        ctx: &egui::Context,
    ) {
        if request.mode == Mode::Thumbnail {
            self.panes
                .entry(Section::Files)
                .or_default()
                .thumbnails
                .finish(&request, result, ctx);
        } else if let Some(preview) = self
            .previews
            .iter_mut()
            .find(|preview| preview.request.id == request.id && !preview.closed)
            && !request.cancel.is_cancelled()
        {
            match result {
                Ok(Output::Image { asset, .. }) => preview.accept(asset, ctx),
                Err(error) => preview.error = format!("图片加载失败: {error}"),
                _ => preview.error = "图片加载失败: 未收到图片数据".into(),
            }
        }
    }
    pub fn open_image(&mut self, path: &str) {
        let pane = self.panes.entry(Section::Files).or_default();
        let items = pane.thumbnails.gallery(&pane.entries);
        if let Some(index) = items.iter().position(|item| item.path == path) {
            self.previews.push(Preview::new(items, index));
        }
    }
    pub fn queue_images(&mut self) {
        self.previews.retain(|preview| !preview.closed);
        let requests = self
            .previews
            .iter_mut()
            .filter(|preview| !preview.queued)
            .map(|preview| {
                preview.queued = true;
                preview.request.clone()
            })
            .collect::<Vec<_>>();
        for request in requests {
            let before = self.queued.len();
            self.enqueue(Section::Files, Operation::ReadImage(request.clone()));
            if before == self.queued.len()
                && let Some(preview) = self
                    .previews
                    .iter_mut()
                    .find(|preview| preview.request.id == request.id)
            {
                preview.queued = false;
            }
        }
        let requests = self
            .panes
            .get_mut(&Section::Files)
            .map(|pane| std::mem::take(&mut pane.thumbnails.queued))
            .unwrap_or_default();
        for request in requests {
            let before = self.queued.len();
            self.enqueue(Section::Files, Operation::ReadImage(request.clone()));
            if before == self.queued.len() {
                self.panes
                    .entry(Section::Files)
                    .or_default()
                    .thumbnails
                    .reject(&request);
            }
        }
    }
}
