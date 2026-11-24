//! Java DialogPositionUtil: owner display's work-area center, then native-size correction.
use egui::{Context, Id, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportId};

#[derive(Clone, Copy)]
struct Pending {
    work_px: Rect,
    attempts: u8,
    last: Option<(f32, Vec2)>,
}
fn key(viewport: ViewportId) -> Id {
    Id::new(("native-dialog-position", viewport))
}
fn valid(rect: Rect) -> bool {
    rect.is_finite() && rect.width() > 0. && rect.height() > 0.
}
fn work_area(ctx: &Context, owner: Option<Rect>) -> Option<Rect> {
    ctx.input(|input| {
        let viewports = &input.raw.viewports;
        if let Some(owner) = owner.filter(|r| valid(*r)) {
            let mut best = None;
            let mut score = 0.;
            for viewport in viewports.values() {
                let Some(rect) = viewport
                    .outer_rect
                    .or(viewport.inner_rect)
                    .filter(|r| valid(*r))
                else {
                    continue;
                };
                let Some(work) = viewport.monitor_work_area_px.filter(|r| valid(*r)) else {
                    continue;
                };
                let intersection_rect = owner.intersect(rect);
                let intersection =
                    intersection_rect.width().max(0.) * intersection_rect.height().max(0.);
                let overlap = intersection / (owner.area() + rect.area() - intersection);
                if overlap > score {
                    score = overlap;
                    best = Some(work);
                }
            }
            if best.is_some() {
                return best;
            }
        }
        viewports
            .values()
            .filter(|v| v.focused == Some(true))
            .find_map(|v| v.monitor_work_area_px.filter(|r| valid(*r)))
            .or_else(|| input.viewport().monitor_work_area_px.filter(|r| valid(*r)))
    })
}
fn centered(work: Rect, size: Vec2) -> Pos2 {
    work.min + (work.size() - size).max(Vec2::ZERO) * 0.5
}
pub(crate) fn owner_rect(ctx: &Context, viewport: ViewportId) -> Option<Rect> {
    ctx.input(|input| {
        input
            .raw
            .viewports
            .get(&viewport)
            .and_then(|v| v.outer_rect.or(v.inner_rect))
    })
}
pub(crate) fn prepare(
    ctx: &Context,
    viewport: ViewportId,
    mut builder: ViewportBuilder,
    first: bool,
    owner: Option<Rect>,
    size: Vec2,
) -> ViewportBuilder {
    if first {
        ctx.data_mut(|data| data.remove::<Pending>(key(viewport)));
        if let Some(work_px) = work_area(ctx, owner) {
            ctx.data_mut(|data| {
                data.insert_temp(
                    key(viewport),
                    Pending {
                        work_px,
                        attempts: 6,
                        last: None,
                    },
                );
            });
            builder = builder.with_position(centered(work_px / ctx.pixels_per_point(), size));
        } else if let Some(owner) = owner {
            builder = builder.with_position(owner.center() - size * 0.5);
        }
    }
    builder
}
pub(crate) fn correct(ui: &Ui, viewport: ViewportId) {
    let ctx = ui.ctx();
    if ctx.viewport_id() != viewport {
        ctx.data_mut(|data| data.remove::<Pending>(key(viewport)));
        return;
    }
    let Some(mut pending) = ctx.data_mut(|data| {
        let pending = data.get_temp::<Pending>(key(viewport));
        data.remove::<Pending>(key(viewport));
        pending
    }) else {
        return;
    };
    if let Some(outer) = ui
        .input(|input| input.viewport().outer_rect)
        .filter(|r| valid(*r))
    {
        let ppp = ctx.pixels_per_point();
        let size_px = outer.size() * ppp;
        let position_px = centered(pending.work_px, size_px);
        let settled = pending.last.is_some_and(|(scale, size)| {
            scale == ppp
                && (size - size_px).abs().max_elem() <= 1.
                && (outer.min.to_vec2() * ppp - position_px.to_vec2())
                    .abs()
                    .max_elem()
                    <= 1.
        });
        if settled {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position_px / ppp));
        pending.last = Some((ppp, size_px));
    }
    if pending.attempts > 1 {
        pending.attempts -= 1;
        ctx.data_mut(|data| data.insert_temp(key(viewport), pending));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests;
