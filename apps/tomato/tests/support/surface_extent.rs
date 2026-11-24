use eframe::egui_wgpu::{self, wgpu};
use std::{num::NonZeroU32, sync::Arc, time::Duration};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::windows::EventLoopBuilderExtWindows as _,
    window::{Window, WindowId},
};

#[test]
#[ignore = "requires an active Windows desktop and a GPU adapter"]
fn actual_client_extent_wins_over_stale_resize_event() {
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("create a native test event loop");
    event_loop.run_app(&mut Fixture).expect("run extent test");
}

struct Fixture;

impl ApplicationHandler for Fixture {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Tomato framebuffer extent test")
                        .with_active(false)
                        .with_inner_size(PhysicalSize::new(320, 240)),
                )
                .expect("create test window"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("create test runtime");
        let viewport = egui::ViewportId::ROOT;
        let mut painter = runtime.block_on(egui_wgpu::winit::Painter::new(
            egui::Context::default(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        runtime
            .block_on(painter.set_window(viewport, Some(window.clone())))
            .expect("initialize the native surface");
        let size = window.inner_size();
        // Model a resize notification one physical pixel behind the window.
        painter.on_window_resized(
            viewport,
            NonZeroU32::new(size.width + 1).unwrap(),
            NonZeroU32::new(size.height + 1).unwrap(),
        );
        painter.paint_and_update_textures(
            viewport,
            window.scale_factor() as f32,
            [1.0; 4],
            &[],
            &mut Default::default(),
            vec![egui::UserData::default()],
            &window,
        );
        painter
            .render_state()
            .unwrap()
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .expect("wait for the surface screenshot");
        let mut events = Vec::new();
        painter.handle_screenshots(&mut events);
        let image = events
            .iter()
            .find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image),
                _ => None,
            })
            .expect("read back the test frame");
        assert_eq!(image.size, [size.width as usize, size.height as usize]);
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}
