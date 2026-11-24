//! Opt-in native driver failure regression; no saved hosts or OS input.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Ready(u8);
impl eframe::App for Ready {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        ui.label("图形初始化回退验证");
        self.0 += 1;
        if self.0 >= 3 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[test]
#[ignore = "requires TOMATO_STARTUP_NATIVE_TEST and an active native desktop"]
fn failed_preferred_device_retries_original_configuration_and_paints() {
    assert!(std::env::var_os("TOMATO_STARTUP_NATIVE_TEST").is_some());
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800., 600.])
            .with_active(false),
        wgpu_preferred_backend: Some(eframe::egui_wgpu::wgpu::Backends::VULKAN),
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        }));
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
    else {
        unreachable!()
    };
    setup.instance_descriptor.backends =
        eframe::egui_wgpu::wgpu::Backends::VULKAN | eframe::egui_wgpu::wgpu::Backends::DX12;
    let original = setup.device_descriptor.clone();
    setup.device_descriptor = Arc::new(move |adapter| {
        let mut descriptor = original(adapter);
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            // Real device request fails. The second request must keep the
            // original factory rather than silently using default settings.
            descriptor.required_features = eframe::egui_wgpu::wgpu::Features::all();
        }
        descriptor
    });
    eframe::run_native(
        "Tomato startup fallback fixture",
        options,
        Box::new(|_| Ok(Box::new(Ready(0)))),
    )
    .expect("Fallback must initialize and paint a real native window");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "Preferred attempt then original configuration"
    );
}
