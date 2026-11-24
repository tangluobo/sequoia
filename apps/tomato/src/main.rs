#![cfg_attr(
    all(target_os = "windows", not(debug_assertions), not(test)),
    windows_subsystem = "windows"
)]

mod capture;
mod render_log;
struct Desktop(tomato_ui_egui::TomatoApp, capture::Capture);
impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
        self.1.update(ctx);
        if self.1.take_close_request() {
            self.0.begin_shutdown();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    fn on_exit(&mut self) {
        if let Some(path) = std::env::var_os("TOMATO_RDP_ACCEPTANCE_REPORT")
            && let Some(report) = self.0.authorized_rdp_diagnostics()
        {
            use std::io::Write as _;
            if let Ok(mut output) = std::fs::File::create_new(path) {
                let _ = output.write_all(report.to_string().as_bytes());
            }
        }
        self.0.begin_shutdown();
    }
}
fn main() -> eframe::Result {
    if let Some(code) = tomato_services::data_import::worker_entry() {
        std::process::exit(code);
    }
    run(None)
}

pub(crate) fn run(event_loop: Option<eframe::EventLoopBuilderHook>) -> eframe::Result {
    render_log::install();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_decorations(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    if let Some(builder) = event_loop {
        options.event_loop_builder = Some(builder);
    }
    // Upstream's default eagerly loads/enumerates every Windows graphics API.
    // Try the same preferred Vulkan path first and retain the original APIs as
    // a fallback if the adapter, device or native surface cannot be initialized.
    // An explicit backend override always wins (including an empty/invalid one).
    if cfg!(target_os = "windows")
        && std::env::var_os("WGPU_BACKEND").is_none()
        && std::env::var_os("WGPU_POWER_PREF").is_none()
    {
        options.wgpu_preferred_backend = Some(eframe::egui_wgpu::wgpu::Backends::VULKAN);
    }
    if std::env::var_os("TOMATO_UI_CAPTURE").is_some() {
        options.viewport = options
            .viewport
            .with_active(false)
            .with_clamp_size_to_monitor_size(false);
    }
    if let Ok(icon) = image::load_from_memory(include_bytes!(
        "../../../crates/ui-egui/assets/images/logo.png"
    )) {
        let icon = icon.thumbnail(128, 128).into_rgba8();
        options.viewport = options.viewport.with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        });
    }
    options.wgpu_options.surface.desired_maximum_frame_latency = Some(1);
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let descriptor = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut descriptor = descriptor(adapter);
            descriptor.memory_hints = eframe::egui_wgpu::wgpu::MemoryHints::MemoryUsage;
            descriptor
        });
    }
    eframe::run_native(
        "Tomato",
        options,
        Box::new(|cc| {
            if let Some(state) = &cc.wgpu_render_state {
                tomato_ui_egui::configure_text_renderer(&cc.egui_ctx, state);
                tomato_ui_egui::enable_gpu_video(&cc.egui_ctx, state.target_format);
                tomato_ui_egui::enable_gpu_rdp(&cc.egui_ctx, state);
            }
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(4)
                .enable_all()
                .build()?;
            Ok(Box::new(Desktop(
                tomato_ui_egui::TomatoApp::new(&cc.egui_ctx, runtime),
                capture::Capture::new(),
            )))
        }),
    )
}
