//! Read-only Windows monitor geometry for owned dialog placement.
#![allow(unsafe_code)]
use winit::platform::windows::MonitorHandleExtWindows as _;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: NativeRect,
    work: NativeRect,
    flags: u32,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
}
pub(super) fn current(window: &winit::window::Window) -> Option<egui::Rect> {
    let monitor = window.current_monitor()?;
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: NativeRect::default(),
        work: NativeRect::default(),
        flags: 0,
    };
    // SAFETY: winit supplies a live HMONITOR; the initialized structure has the
    // documented MONITORINFO layout and is exclusively writable for this call.
    if unsafe { GetMonitorInfoW(monitor.hmonitor(), &mut info) } == 0 {
        return None;
    }
    let work = info.work;
    (work.left < work.right && work.top < work.bottom).then(|| {
        egui::Rect::from_min_max(
            egui::pos2(work.left as f32, work.top as f32),
            egui::pos2(work.right as f32, work.bottom as f32),
        )
    })
}
