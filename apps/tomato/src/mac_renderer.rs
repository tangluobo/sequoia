use std::cell::Cell;

use eframe::{Error, NativeOptions, Renderer, egui_wgpu::WgpuError};

pub(super) fn run(options: NativeOptions) -> eframe::Result {
    let mut fallback = options.clone();
    fallback.renderer = Renderer::Glow;
    if std::env::var("TOMATO_RENDERER").as_deref() == Ok("opengl") {
        fallback.event_loop_builder = options.event_loop_builder;
        fallback.window_builder = options.window_builder;
        return eframe::run_native("Tomato", fallback, Box::new(super::create_desktop));
    }

    let created = Cell::new(false);
    let result = eframe::run_native(
        "Tomato",
        options,
        Box::new(|cc| {
            created.set(true);
            super::create_desktop(cc)
        }),
    );
    if !created.get()
        && let Err(Error::Wgpu(
            error @ (WgpuError::RequestAdapterError(_)
            | WgpuError::RequestDeviceError(_)
            | WgpuError::CreateSurfaceError(_)
            | WgpuError::NoSurfaceFormatsAvailable),
        )) = result
    {
        eprintln!("Metal startup failed ({error}), retrying with OpenGL.");
        // eframe's run-and-return mode keeps the main thread's winit loop for
        // the second renderer. The application is created only after it starts.
        return eframe::run_native("Tomato", fallback, Box::new(super::create_desktop));
    }
    result
}
