//! Standalone, owned native-window regression for application-owned viewports.
//! No saved profiles, networking, clipboard access or operating-system input.
use eframe::egui;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Proof {
    root_passes: usize,
    child_passes: usize,
    child_minimized_logic: usize,
    close_seen: bool,
    finished: bool,
}
struct Probe {
    started: Instant,
    phase: u8,
    phase_at: Instant,
    proof: Arc<Mutex<Proof>>,
}
fn child() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("owned-rdp-viewport-regression")
}
impl eframe::App for Probe {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let elapsed = self.started.elapsed();
        let is_child = ctx.viewport_id() == child();
        if is_child && ctx.input(|input| input.viewport().minimized == Some(true)) {
            self.proof.lock().unwrap().child_minimized_logic += 1;
        }
        if is_child && ctx.input(|input| input.viewport().close_requested()) {
            self.proof.lock().unwrap().close_seen = true;
            self.phase = 4;
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let step = self.phase_at.elapsed();
        if step >= Duration::from_millis(300)
            && self.phase == 0
            && self.proof.lock().unwrap().child_passes > 0
        {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(true),
            );
            self.phase = 1;
            self.phase_at = Instant::now();
        } else if step >= Duration::from_millis(300) && self.phase == 1 {
            ctx.send_viewport_cmd_to(child(), egui::ViewportCommand::Minimized(true));
            self.phase = 2;
            self.phase_at = Instant::now();
        } else if step >= Duration::from_millis(400) && self.phase == 2 {
            ctx.send_viewport_cmd_to(child(), egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(false),
            );
            self.phase = 3;
            self.phase_at = Instant::now();
        } else if step >= Duration::from_millis(400) && self.phase == 3 {
            ctx.send_viewport_cmd_to(child(), egui::ViewportCommand::Close);
            self.phase = 5;
            self.phase_at = Instant::now();
        } else if step >= Duration::from_millis(600) && self.phase == 4 {
            self.proof.lock().unwrap().finished = true;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        if elapsed >= Duration::from_secs(20) {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(20));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if ui.ctx().viewport_id() == child() {
            self.proof.lock().unwrap().child_passes += 1;
            ui.label("Owned RDP viewport regression");
        } else {
            self.proof.lock().unwrap().root_passes += 1;
            ui.label("Owned parent: child close must leave this window alive");
            if self.phase != 4 {
                assert!(
                    ui.ctx().show_viewport_app(
                        child(),
                        egui::ViewportBuilder::default()
                            .with_title("Owned RDP viewport regression")
                            .with_active(false)
                            .with_inner_size([360., 200.])
                    )
                );
            }
        }
    }
}
fn main() {
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).expect("fresh proof path"));
    assert!(!output.exists(), "proof must be fresh");
    let proof = Arc::new(Mutex::new(Proof::default()));
    let state = proof.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_active(false)
            .with_inner_size([420., 220.]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Owned RDP parent regression",
        options,
        Box::new(move |_| {
            Ok(Box::new(Probe {
                started: Instant::now(),
                phase: 0,
                phase_at: Instant::now(),
                proof: state,
            }))
        }),
    )
    .expect("owned native event loop");
    let result = proof.lock().unwrap();
    let json = format!(
        "{{\"rootPasses\":{},\"childPasses\":{},\"childMinimizedLogic\":{},\"childCloseSeen\":{},\"rootSurvivedChildClose\":{},\"savedHostsContacted\":false}}\n",
        result.root_passes,
        result.child_passes,
        result.child_minimized_logic,
        result.close_seen,
        result.finished
    );
    use std::io::Write;
    std::fs::File::create_new(output)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    assert!(
        result.root_passes > 0
            && result.child_passes > 0
            && result.child_minimized_logic > 0
            && result.close_seen
            && result.finished,
        "native viewport lifecycle failed: {json}"
    );
    print!("{json}");
}
