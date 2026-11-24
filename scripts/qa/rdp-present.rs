//! Owned native presentation experiment, without profiles/network/OS input.
use eframe::{
    egui,
    egui_wgpu::{RenderState, wgpu},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Metrics {
    upload_seconds: f64,
    max_upload_ms: f64,
    count: usize,
    first: Option<Instant>,
    last: Option<Instant>,
    gap: f64,
    finished: bool,
}
struct Probe {
    output: std::path::PathBuf,
    mode: String,
    start: Instant,
    state: RenderState,
    texture: wgpu::Texture,
    id: egui::TextureId,
    pixels: [Vec<u8>; 2],
    tick: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    last: usize,
    metrics: Arc<Mutex<Metrics>>,
    window: bool,
}
fn child() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("owned-present-probe")
}
impl eframe::App for Probe {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.start.elapsed() > Duration::from_secs(7) {
            let mut metrics = self.metrics.lock().unwrap();
            if !metrics.finished {
                metrics.finished = true;
                let seconds = metrics
                    .last
                    .unwrap()
                    .duration_since(metrics.first.unwrap())
                    .as_secs_f64();
                let proof = format!(
                    "{{\"mode\":{:?},\"window\":{},\"updates\":{},\"seconds\":{},\"updatesPerSecond\":{},\"maxGapMs\":{},\"scope\":\"Owned GPU upload/presentation without decoding\"}}\n",
                    self.mode,
                    self.window,
                    metrics.count,
                    seconds,
                    (metrics.count - 1) as f64 / seconds,
                    metrics.gap
                );
                std::fs::write(&self.output, proof).unwrap();
                eprintln!("presentation sampling completed");
            }
            drop(metrics);
            self.stop.store(true, Ordering::Release);
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.window && ui.ctx().viewport_id() == egui::ViewportId::ROOT {
            ui.label("Owned presentation parent");
            ui.ctx().show_viewport_app(
                child(),
                egui::ViewportBuilder::default()
                    .with_title("Owned presentation child")
                    .with_inner_size([1920., 1080.])
                    .with_active(false),
            );
            ui.ctx().request_repaint_after(Duration::from_millis(250));
            return;
        }
        let tick = self.tick.load(Ordering::Acquire);
        if tick != self.last {
            let upload_start = Instant::now();
            self.state.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.pixels[tick % 2],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1920 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
            );
            self.last = tick;
            if self.start.elapsed() >= Duration::from_secs(1) {
                let now = Instant::now();
                let mut m = self.metrics.lock().unwrap();
                if m.finished {
                    return;
                }
                let upload = upload_start.elapsed().as_secs_f64();
                m.upload_seconds += upload;
                m.max_upload_ms = m.max_upload_ms.max(upload * 1000.);
                m.first.get_or_insert(now);
                if let Some(last) = m.last {
                    m.gap = m.gap.max(now.duration_since(last).as_secs_f64() * 1000.);
                }
                m.last = Some(now);
                m.count += 1;
            }
        }
        ui.painter().image(
            self.id,
            ui.max_rect(),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        eprintln!("drop: stop source");
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
        eprintln!("drop: unregister texture");
        self.state.renderer.write().free_texture(&self.id);
        eprintln!("drop: done");
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).expect("fifo1/fifo2/mailbox");
    let window = args.get(2).expect("TAB/WINDOW") == "WINDOW";
    let output = std::path::PathBuf::from(args.get(3).expect("fresh proof path"));
    assert!(!output.exists());
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Owned presentation")
            .with_inner_size(if window { [420., 220.] } else { [1920., 1080.] })
            .with_active(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    if args.get(4).is_some_and(|value| value == "usage")
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
    {
        let descriptor = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut device = descriptor(adapter);
            device.memory_hints = wgpu::MemoryHints::MemoryUsage;
            device
        });
    }
    options.wgpu_options.surface.desired_maximum_frame_latency =
        Some(if mode == "fifo2" { 2 } else { 1 });
    if mode == "mailbox" {
        options.wgpu_options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    let result = metrics.clone();
    let output_copy = output.clone();
    let mode_copy = mode.clone();
    eframe::run_native(
        "Owned RDP presentation",
        options,
        Box::new(move |cc| {
            let state = cc.wgpu_render_state.as_ref().unwrap().clone();
            eprintln!("adapter={:?}", state.adapter.get_info());
            let texture = state.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Owned full RGBA upload"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let id = state.renderer.write().register_native_texture(
                &state.device,
                &view,
                wgpu::FilterMode::Nearest,
            );
            let pixels = [12, 80].map(|red| [red, 34, 56, 255].repeat(1920 * 1080));
            let tick = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let source = tick.clone();
            let cancel = stop.clone();
            let ctx = cc.egui_ctx.clone();
            let target = if window {
                child()
            } else {
                egui::ViewportId::ROOT
            };
            let thread = std::thread::spawn(move || {
                let mut next = Instant::now();
                while !cancel.load(Ordering::Acquire) {
                    source.fetch_add(1, Ordering::Release);
                    ctx.request_repaint_of(target);
                    next += Duration::from_nanos(16_666_667);
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            });
            Ok(Box::new(Probe {
                output: output_copy,
                mode: mode_copy,
                start: Instant::now(),
                state,
                texture,
                id,
                pixels,
                tick,
                stop,
                thread: Some(thread),
                last: usize::MAX,
                metrics,
                window,
            }))
        }),
    )
    .unwrap();
    let m = result.lock().unwrap();
    let seconds = m
        .last
        .unwrap()
        .duration_since(m.first.unwrap())
        .as_secs_f64();
    let json = format!(
        "{{\"mode\":{mode:?},\"window\":{window},\"updates\":{},\"seconds\":{seconds},\"updatesPerSecond\":{},\"maxGapMs\":{},\"averageUploadMs\":{},\"maxUploadMs\":{},\"finished\":{},\"scope\":\"Owned native GPU upload/presentation, without decoding\"}}\n",
        m.count,
        (m.count - 1) as f64 / seconds,
        m.gap,
        m.upload_seconds * 1000. / m.count as f64,
        m.max_upload_ms,
        m.finished
    );
    use std::io::Write;
    std::fs::File::create(output)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    print!("{json}");
    assert!(m.finished);
}
