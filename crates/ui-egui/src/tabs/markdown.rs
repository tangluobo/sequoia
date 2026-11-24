use std::collections::{HashMap, HashSet};
use tomato_services::files::images::{Animation, DocumentGif};
pub(crate) mod search_job;
pub fn trim_undo(ctx: &egui::Context, id: egui::Id) {
    if let Some(state) = egui::widgets::text_edit::TextEditState::load(ctx, id) {
        state.trim_undo_memory(32 * 1024 * 1024);
    }
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Edit,
    #[default]
    Split,
    Preview,
}
pub enum Picture {
    Pending,
    Ready(ReadyPicture),
    Failed(String),
}
const PICTURE_CACHE: usize = 16 * 1024 * 1024;

pub struct ReadyPicture {
    pub texture: egui::TextureHandle,
    motion: Option<Box<Motion>>,
}

struct Motion {
    source: DocumentGif,
    playback: Option<Animation>,
    finished: bool,
}

impl ReadyPicture {
    pub(crate) fn new(
        ctx: &egui::Context,
        mut picture: tomato_services::markdown::Picture,
    ) -> Self {
        let motion = picture
            .gif
            .take()
            .filter(|gif| gif.retained_bytes() + picture.rgba.len() <= PICTURE_CACHE)
            .map(|source| {
                Box::new(Motion {
                    source,
                    playback: None,
                    finished: false,
                })
            });
        let texture = ctx.load_texture(
            "Markdown picture",
            pixels(&picture),
            egui::TextureOptions::LINEAR,
        );
        Self { texture, motion }
    }

    fn update_motion(&mut self, ctx: &egui::Context, visible: bool) {
        let Some(motion) = &mut self.motion else {
            return;
        };
        if !visible {
            motion.playback = None;
            return;
        }
        if let Some(playback) = &motion.playback {
            let finished = playback.is_finished();
            if let Some(frame) = playback.frame() {
                self.texture
                    .set(pixels(&frame), egui::TextureOptions::LINEAR);
            }
            if finished {
                motion.finished = true;
                motion.playback = None;
            }
        } else if !motion.finished {
            let wake = ctx.clone();
            match motion
                .source
                .try_start(move || wake.request_repaint_of(egui::ViewportId::ROOT))
            {
                Ok(playback) => motion.playback = playback,
                Err(_) => motion.finished = true,
            }
        }
        if motion.finished {
            self.motion = None;
        }
    }

    pub(crate) fn is_animated(&self) -> bool {
        self.motion.is_some()
    }

    fn retained_bytes(&self) -> usize {
        self.texture.size()[0] * self.texture.size()[1] * 4
            + self
                .motion
                .as_ref()
                .map_or(0, |motion| motion.source.retained_bytes())
    }
}

fn pixels(picture: &tomato_services::markdown::Picture) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [picture.width as usize, picture.height as usize],
        &picture.rgba,
    )
}
pub struct State {
    pub rectangle: RectangleState,
    pub remote: Option<std::sync::Arc<tomato_core::Connection>>,
    pub file_embedded: bool,
    pub file_owner: Option<String>,
    pub file_client: Option<tomato_services::files::document::Client>,
    pub editor_id: Option<egui::Id>,
    pub find_field_ids: [Option<egui::Id>; 3],
    pub mode: Mode,
    pub ratio: f32,
    pub revision: u64,
    pub changed: f64,
    pub pending: bool,
    pub document: tomato_services::markdown::Document,
    pub parsed: Option<u64>,
    pub heights: Vec<f32>,
    pub(crate) table_layouts: HashMap<usize, TableGeometry>,
    pub width: f32,
    pub images: HashMap<String, Picture>,
    pub(crate) picture_frame: Option<u64>,
    pub find: bool,
    pub find_focus: bool,
    pub find_positioned: bool,
    pub reveal_match: bool,
    pub needle: String,
    pub replacement: String,
    pub match_index: usize,
    pub match_case: bool,
    pub regex: bool,
    pub replace_visible: bool,
    pub search_key: Option<search_job::Key>,
    pub search: Option<Result<std::sync::Arc<tomato_core::search::Search>, String>>,
    pub search_job: Option<search_job::Job>,
    pub find_notice: Option<search_job::Notice>,
    pub find_advance: bool,
    pub status: String,
    pub close_after_save: bool,
    pub resource_generation: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            rectangle: RectangleState::default(),
            remote: None,
            file_client: None,
            file_owner: None,
            file_embedded: false,
            editor_id: None,
            find_field_ids: [None; 3],
            mode: Mode::Split,
            ratio: 0.5,
            revision: 0,
            changed: 0.,
            pending: false,
            document: Default::default(),
            parsed: None,
            heights: Vec::new(),
            table_layouts: HashMap::new(),
            width: 0.,
            images: HashMap::new(),
            picture_frame: None,
            find: false,
            find_focus: false,
            find_positioned: false,
            reveal_match: false,
            needle: String::new(),
            replacement: String::new(),
            match_index: 0,
            match_case: true,
            regex: false,
            replace_visible: false,
            search_key: None,
            search: None,
            search_job: None,
            find_notice: None,
            find_advance: false,
            status: String::new(),
            close_after_save: false,
            resource_generation: 0,
        }
    }
}

#[derive(Default)]
pub(crate) struct TableGeometry {
    pub available: f32,
    pub scale: f32,
    pub bold_cjk: bool,
    pub widths: Vec<f32>,
    pub row_ends: Vec<f32>,
}

#[derive(Default)]
pub struct RectangleState {
    pub selection: Option<tomato_core::markdown_edit::Rectangle>,
    pub dragging: bool,
    pub geometry: Option<RectangleGeometry>,
}
pub struct RectangleGeometry {
    pub galley: std::sync::Arc<egui::Galley>,
    pub origin: egui::Pos2,
    pub clip: egui::Rect,
}
impl State {
    pub fn changed(&mut self, now: f64) {
        self.revision = self.revision.wrapping_add(1);
        self.changed = now;
    }
    pub(crate) fn retain_image_sources(&mut self, document: &tomato_services::markdown::Document) {
        if self.images.is_empty() {
            return;
        }
        let sources: HashSet<_> = document
            .images()
            .map(|image| image.source.as_str())
            .collect();
        self.images
            .retain(|source, _| sources.contains(source.as_str()));
    }
    pub fn image(
        &mut self,
        ctx: &egui::Context,
        key: String,
        picture: tomato_services::markdown::Picture,
    ) {
        self.table_layouts.clear();
        let picture = ReadyPicture::new(ctx, picture);
        let incoming = picture.retained_bytes();
        let held: usize = self
            .images
            .values()
            .map(|image| match image {
                Picture::Ready(picture) => picture.retained_bytes(),
                _ => 0,
            })
            .sum();
        if held.saturating_add(incoming) > PICTURE_CACHE {
            self.images
                .retain(|_, image| matches!(image, Picture::Pending));
        }
        self.images.insert(key, Picture::Ready(picture));
    }

    pub(crate) fn image_failed(&mut self, source: String, error: String) {
        self.table_layouts.clear();
        self.images.insert(source, Picture::Failed(error));
    }

    pub(crate) fn clear_images(&mut self) {
        self.table_layouts.clear();
        self.images.clear();
    }

    pub(crate) fn animate_visible(
        &mut self,
        ctx: &egui::Context,
        visible: &HashSet<egui::TextureId>,
    ) {
        if ctx.input(|input| input.viewport().minimized == Some(true)) {
            return;
        }
        self.picture_frame = Some(ctx.cumulative_frame_nr());
        for picture in self.images.values_mut() {
            if let Picture::Ready(picture) = picture {
                picture.update_motion(ctx, visible.contains(&picture.texture.id()));
            }
        }
    }

    pub(crate) fn hide_pictures(&mut self, ctx: &egui::Context, minimized: bool) {
        if self
            .picture_frame
            .is_some_and(|frame| minimized || frame != ctx.cumulative_frame_nr())
        {
            self.picture_frame = None;
            for picture in self.images.values_mut() {
                if let Picture::Ready(picture) = picture {
                    picture.update_motion(ctx, false);
                }
            }
        }
    }
}
