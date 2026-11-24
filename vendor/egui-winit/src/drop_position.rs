//! Native file drops can arrive without CursorMoved during Windows OLE drags.
#![allow(unsafe_code)]
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut Point) -> i32;
}

pub(super) fn current(window: &winit::window::Window) -> Option<winit::dpi::PhysicalPosition<f64>> {
    let mut point = Point { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes one initialized POINT with the Windows ABI.
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    let origin = window.inner_position().ok()?;
    Some(winit::dpi::PhysicalPosition::new(
        f64::from(point.x) - f64::from(origin.x),
        f64::from(point.y) - f64::from(origin.y),
    ))
}
