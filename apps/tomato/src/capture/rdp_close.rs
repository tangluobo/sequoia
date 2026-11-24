//! Opt-in acceptance uses this application's viewport commands, never OS input injection.
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) struct Closing {
    child: egui::ViewportId,
    path: PathBuf,
    started: Instant,
    requested: Option<Instant>,
    minimized: bool,
    minimized_seen: bool,
    observed: Option<(egui::ViewportId, bool, bool, bool)>,
}
impl Closing {
    pub(super) fn start(ctx: &egui::Context, path: &Path, child: egui::ViewportId) -> Option<Self> {
        if child == egui::ViewportId::ROOT
            || std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("rdp")
        {
            return None;
        }
        let minimized = match std::env::var("TOMATO_RDP_CAPTURE_CLOSE").as_deref() {
            Ok("WINDOW") => false,
            Ok("MINIMIZED") => true,
            _ => return None,
        };
        let mut state = Self {
            child,
            path: path.to_owned(),
            started: Instant::now(),
            requested: None,
            minimized,
            minimized_seen: false,
            observed: None,
        };
        if minimized {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(true),
            );
            ctx.send_viewport_cmd_to(child, egui::ViewportCommand::Minimized(true));
        } else {
            state.request(ctx);
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        Some(state)
    }
    fn request(&mut self, ctx: &egui::Context) {
        self.requested = Some(Instant::now());
        ctx.send_viewport_cmd_to(self.child, egui::ViewportCommand::Close);
    }
    pub(super) fn update(&mut self, ctx: &egui::Context) -> bool {
        let (present, root_alive, all_minimized) = ctx.input(|input| {
            let root = input.raw.viewports.get(&egui::ViewportId::ROOT);
            let child = input.raw.viewports.get(&self.child);
            (
                child.is_some(),
                root.is_some(),
                root.is_some_and(|v| v.minimized == Some(true))
                    && child.is_some_and(|v| v.minimized == Some(true)),
            )
        });
        let observation = (ctx.viewport_id(), present, root_alive, all_minimized);
        if self.observed != Some(observation) {
            eprintln!("RDP close observation: {observation:?}");
            self.observed = Some(observation);
        }
        if self.requested.is_none() && all_minimized {
            self.minimized_seen = true;
            self.request(ctx);
        }
        let closed = self.requested.is_some() && !present;
        if closed || self.started.elapsed() > Duration::from_secs(6) {
            let elapsed = self
                .requested
                .map(|at| at.elapsed().as_millis().to_string())
                .unwrap_or_else(|| "null".into());
            let success = closed && root_alive && (!self.minimized || self.minimized_seen);
            let data = format!(
                r#"{{"success":{success},"childRemoved":{closed},"rootAlive":{root_alive},"minimizedRequested":{},"bothMinimizedObserved":{},"closeToRemovalMillis":{elapsed},"osInputInjected":false}}"#,
                self.minimized, self.minimized_seen
            );
            if let Err(error) = std::fs::write(self.path.with_extension("child-close.json"), data) {
                eprintln!("Child close acceptance report failed: {error}");
            }
            let _ = std::fs::write(self.path.with_extension("close"), []);
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            return true;
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        ctx.request_repaint_after(Duration::from_millis(20));
        false
    }
}
