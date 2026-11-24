//! Opt-in capture uses the real application composition root on a test thread.
#[allow(dead_code)]
#[path = "../src/main.rs"]
mod program;
#[test]
#[ignore = "requires TOMATO_UI_CAPTURE and a native desktop renderer"]
fn capture_desktop() {
    assert!(
        std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
        "Capture path is required"
    );
    #[cfg(target_os = "windows")]
    let hook: Option<eframe::EventLoopBuilderHook> = Some(Box::new(|builder| {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }));
    #[cfg(not(target_os = "windows"))]
    let hook = None;
    program::run(hook).expect("Desktop capture failed");
}

#[path = "support/offscreen.rs"]
mod offscreen;
#[test]
#[ignore = "requires TOMATO_UI_CAPTURE and a GPU adapter; does not validate a native window"]
fn capture_offscreen() -> Result<(), Box<dyn std::error::Error>> {
    offscreen::capture()
}

#[cfg(target_os = "windows")]
#[path = "support/surface_extent.rs"]
mod surface_extent;
