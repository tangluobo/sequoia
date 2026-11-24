pub mod crop;
pub struct State {
    pub options: tomato_services::tools::icon::Options,
    pub job: Option<tomato_services::tools::icon::job::Job>,
    pub original: Option<egui::TextureHandle>,
    pub processed: Option<egui::TextureHandle>,
    pub sizes: [[u32; 2]; 2],
    pub crop: crop::Crop,
    pub fields: [String; 4],
    pub directory: bool,
    pub revision: u64,
    pub preview_revision: Option<u64>,
    pub job_revision: u64,
    pub source_hash: Option<[u8; 32]>,
    pub dirty_at: Option<std::time::Instant>,
    pub status: String,
    pub drag: Option<(u8, egui::Pos2, [u32; 4])>,
    pub image_rect: Option<egui::Rect>,
    pub scroll_offset: f32,
    pub content_height: f32,
    pub scrollbar: crate::native_scrollbar::State,
    pub error: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            options: Default::default(),
            job: None,
            original: None,
            processed: None,
            sizes: [[0; 2]; 2],
            crop: Default::default(),
            fields: std::array::from_fn(|_| "0".into()),
            directory: true,
            revision: 0,
            preview_revision: None,
            job_revision: 0,
            source_hash: None,
            dirty_at: None,
            status: String::new(),
            drag: None,
            image_rect: None,
            scroll_offset: 0.,
            content_height: 0.,
            scrollbar: Default::default(),
            error: false,
        }
    }
}
impl State {
    pub fn sync_fields(&mut self) {
        self.fields = self.crop.margins().map(|v| v.to_string());
    }
    pub fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.dirty_at = Some(std::time::Instant::now());
        self.preview_revision = None;
        self.source_hash = None;
    }
    pub fn ready(&self) -> bool {
        self.job.is_none()
            && self.dirty_at.is_none()
            && self.preview_revision == Some(self.revision)
    }
}
