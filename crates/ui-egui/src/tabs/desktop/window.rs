//! Native viewport state belongs to the session, not to a second protocol connection.
pub struct Window {
    pub restore_tab: bool,
    pub fullscreen: bool,
    pub initialized: bool,
    pub focused: bool,
    pub scrollbars: bool,
    pub resize_after: f64,
    pub last_size: Option<egui::Vec2>,
    pub pinned: bool,
    pub bar_until: f64,
    pub bar_x: f32,
    pub quality: bool,
    pub alert_focus: bool,
    pub last_status: Option<tomato_services::remote_desktop::Status>,
    pub blocked: bool,
}
impl Window {
    pub fn enter_fullscreen(&mut self, now: f64) {
        self.fullscreen = true;
        self.pinned = false;
        self.bar_x = 0.;
        self.quality = false;
        self.bar_until = now + 5.;
    }
    pub fn restore(&mut self) {
        self.fullscreen = false;
        self.scrollbars = true;
        self.pinned = false;
        self.bar_x = 0.;
        self.quality = false;
        self.bar_until = 0.;
    }
    pub fn new(restore_tab: bool, fullscreen: bool) -> Self {
        Self {
            restore_tab,
            fullscreen,
            initialized: false,
            focused: false,
            scrollbars: restore_tab,
            resize_after: f64::INFINITY,
            last_size: None,
            pinned: false,
            bar_until: 0.,
            bar_x: 0.,
            quality: false,
            alert_focus: true,
            last_status: None,
            blocked: false,
        }
    }
}
