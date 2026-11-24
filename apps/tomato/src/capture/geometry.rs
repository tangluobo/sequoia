//! Opt-in, read-only native viewport geometry for framebuffer acceptance probes.
use std::path::Path;
pub(super) fn record(ctx: &egui::Context, target: egui::ViewportId, path: &Path) {
    let zoom = ctx.zoom_factor();
    let rows = ctx.input(|input| input.raw.viewports.iter().filter_map(|(id, info)| {
        let outer = info.outer_rect?;
        let work = info.monitor_work_area_px?;
        let ppp = info.native_pixels_per_point? * zoom;
        if !outer.is_finite() || !work.is_finite() || !ppp.is_finite() || ppp <= 0. { return None; }
        let outer = outer * ppp;
        Some(format!("{{\"target\":{},\"root\":{},\"scale\":{},\"outerPx\":[{},{},{},{}],\"workPx\":[{},{},{},{}]}}", *id == target, *id == egui::ViewportId::ROOT, ppp, outer.min.x, outer.min.y, outer.max.x, outer.max.y, work.min.x, work.min.y, work.max.x, work.max.y))
    }).collect::<Vec<_>>());
    let _ = std::fs::write(
        path.with_extension("geometry.json"),
        format!("[{}]", rows.join(",")),
    );
}
