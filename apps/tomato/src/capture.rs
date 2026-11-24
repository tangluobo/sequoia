//! Opt-in framebuffer capture for reproducible visual checks of this application.
mod geometry;
mod rdp_close;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Capture {
    path: Option<PathBuf>,
    started: Instant,
    requested: bool,
    frames: u8,
    index: u8,
    next_request: Instant,
    viewport: Option<egui::ViewportId>,
    attempts: u8,
    held: Option<PathBuf>,
    closing_rdp: Option<rdp_close::Closing>,
    root_close: bool,
}
impl Capture {
    pub fn new() -> Self {
        let started = Instant::now();
        let default_delay = if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("ssh") {
            4000
        } else {
            700
        };
        let delay = std::env::var("TOMATO_UI_CAPTURE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v.clamp(700, 10000))
            .unwrap_or(default_delay);
        Self {
            // The isolated manual upload fixture uses CAPTURE to install its preview,
            // but closes through its own completion marker after real user input.
            path: std::env::var_os("TOMATO_UI_CAPTURE")
                .filter(|_| {
                    !(std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("file_drop")
                        && std::env::var("TOMATO_FILE_DROP_CASE").as_deref() == Ok("manual"))
                })
                .map(PathBuf::from),
            started,
            requested: false,
            frames: std::env::var("TOMATO_UI_CAPTURE_FRAMES")
                .ok()
                .and_then(|value| value.parse::<u8>().ok())
                .unwrap_or(1)
                .clamp(1, 8),
            index: 0,
            next_request: started + Duration::from_millis(delay),
            viewport: None,
            attempts: 0,
            held: None,
            closing_rdp: None,
            root_close: false,
        }
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        if let Some(closing) = &mut self.closing_rdp {
            if closing.update(ctx) {
                self.closing_rdp = None;
            }
            return;
        }
        if let Some(path) = self.held.as_ref() {
            // Only the explicitly authorized, owned saved-host acceptance mode
            // waits for a cooperative stop marker while a human moves the mouse.
            if path.with_extension("stop").is_file() {
                self.held = None;
                self.root_close = true;
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("markdown_print")
            && !ctx
                .data(|data| data.get_temp::<bool>(egui::Id::new("owned-markdown-print-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("markdown_html")
            && !ctx
                .data(|data| data.get_temp::<bool>(egui::Id::new("owned-markdown-html-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("json_selection")
            && !ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new("owned-json-selection-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("image_format_converter" | "dataset_converter")
        ) && std::env::var("TOMATO_UI_CONVERTER_RUN").as_deref() == Ok("owned-files")
            && !ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new("owned-converter-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("database_recovery")
            && !ctx
                .data(|data| data.get_temp::<bool>(egui::Id::new("owned-recovery-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("data_import")
            && !ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new("owned-import-ready")))
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let viewport = ctx
            .data(|data| {
                data.get_temp::<egui::ViewportId>(egui::Id::new("tomato-capture-viewport"))
            })
            .unwrap_or(egui::ViewportId::ROOT);
        if self.viewport != Some(viewport) {
            self.viewport = Some(viewport);
            self.requested = false;
            self.attempts = 0;
            self.next_request = self
                .next_request
                .max(Instant::now() + Duration::from_millis(700));
            if std::env::var_os("TOMATO_CAPTURE_TRACE").is_some() {
                eprintln!("Capture target changed: {viewport:?}");
            }
            ctx.data_mut(|data| {
                data.remove::<std::sync::Arc<egui::ColorImage>>(egui::Id::new(
                    "tomato-capture-image",
                ))
            });
        }
        let screenshot = ctx
            .data_mut(|data| {
                let id = egui::Id::new("tomato-capture-image");
                let image = data.get_temp::<std::sync::Arc<egui::ColorImage>>(id);
                data.remove::<std::sync::Arc<egui::ColorImage>>(id);
                image
            })
            .or_else(|| {
                ctx.input(|input| {
                    input.events.iter().find_map(|event| {
                        if let egui::Event::Screenshot {
                            viewport_id, image, ..
                        } = event
                            && *viewport_id == viewport
                        {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                })
            });
        if let Some(image) = screenshot {
            geometry::record(ctx, viewport, &path);
            if let Some(sample) = ctx.data(|data| {
                data.get_temp::<[f64; 5]>(egui::Id::new("tomato-adb-video-capture-metrics"))
            }) {
                let duration = sample[2] - sample[1];
                let format = ctx
                    .data(|data| data.get_temp::<String>(egui::Id::new("tomato-adb-video-format")))
                    .unwrap_or_else(|| "RGBA".into());
                let json = format!(
                    "{{\"pixelFormat\":\"{}\",\"textureUpdates\":{},\"durationSeconds\":{},\"updatesPerSecond\":{},\"maxUpdateGapMs\":{},\"scope\":\"owned synthetic H264 source; GPU texture submissions, not physical display scanout\"}}\n",
                    format,
                    sample[3],
                    duration,
                    if duration > 0. {
                        (sample[3] - 1.) / duration
                    } else {
                        0.
                    },
                    sample[4] * 1000.
                );
                let _ = std::fs::write(path.with_extension("video.json"), json);
            }
            if let Some(sample) = ctx
                .data(|data| data.get_temp::<[f64; 7]>(egui::Id::new("tomato-rdp-capture-metrics")))
            {
                let duration = sample[2] - sample[1];
                let json = format!(
                    "{{\"pixelFormat\":\"native-RGBA\",\"uiUpdates\":{},\"durationSeconds\":{},\"updatesPerSecond\":{},\"maxUpdateGapMs\":{},\"uploadedPixels\":{},\"patches\":{},\"scope\":\"owned RDP loopback; UI texture updates, not physical scanout or network end-to-end latency\"}}",
                    sample[3],
                    duration,
                    if duration > 0. {
                        (sample[3] - 1.) / duration
                    } else {
                        0.
                    },
                    sample[4] * 1000.,
                    sample[5],
                    sample[6]
                );
                let _ = std::fs::write(path.with_extension("rdp.json"), json);
            }
            if std::env::var_os("TOMATO_CAPTURE_TRACE").is_some() {
                eprintln!(
                    "Capture image {}: {}x{}",
                    self.index,
                    image.width(),
                    image.height()
                );
            }
            let destination = if self.index == 0 {
                path.clone()
            } else {
                path.with_file_name(format!(
                    "{}-{:02}.png",
                    path.file_stem().unwrap_or_default().to_string_lossy(),
                    self.index
                ))
            };
            let bytes: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            if let Err(error) = image::save_buffer_with_format(
                destination,
                &bytes,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            ) {
                eprintln!("Framebuffer capture failed: {error}");
            }
            self.index += 1;
            if self.index >= self.frames {
                let destination = path.clone();
                let hold = std::env::var("TOMATO_TEST_SAVED_110").as_deref() == Ok("authorized")
                    && std::env::var("TOMATO_RDP_CAPTURE_HOLD").as_deref() == Ok("1");
                self.path = None;
                if hold {
                    let _ = std::fs::write(destination.with_extension("holding"), []);
                    self.held = Some(destination);
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                if let Some(closing) = rdp_close::Closing::start(ctx, &destination, viewport) {
                    self.closing_rdp = Some(closing);
                    return;
                }
                // The native capture harness times shutdown from this marker,
                // after PNG encoding/writing has finished, rather than from the
                // destination file's first appearance on a slow disk.
                if let Err(error) = std::fs::write(destination.with_extension("close"), []) {
                    eprintln!("Capture close marker failed: {error}");
                }
                if std::env::var_os("TOMATO_CAPTURE_TRACE").is_some() {
                    eprintln!(
                        "Capture finished in {:?}; requesting root viewport close from {:?}",
                        self.started.elapsed(),
                        ctx.viewport_id()
                    );
                }
                self.root_close = true;
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                self.requested = false;
                self.attempts = 0;
                self.next_request = Instant::now() + Duration::from_millis(250);
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        } else if self.started.elapsed() > Duration::from_secs(30) {
            eprintln!("Framebuffer capture timed out");
            self.path = None;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            if self.requested && self.attempts < 3 && Instant::now() >= self.next_request {
                self.requested = false;
            }
            let ready = std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("rdp")
                || ctx
                    .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-rdp-first-frame")))
                    .unwrap_or(false);
            if ready && !self.requested && Instant::now() >= self.next_request {
                ctx.send_viewport_cmd_to(
                    viewport,
                    egui::ViewportCommand::Screenshot(egui::UserData::default()),
                );
                self.requested = true;
                if std::env::var_os("TOMATO_CAPTURE_TRACE").is_some() {
                    eprintln!("Capture request: {viewport:?}");
                }
                self.attempts += 1;
                self.next_request = Instant::now() + Duration::from_secs(2);
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.root_close)
    }
}
