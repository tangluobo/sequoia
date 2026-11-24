//! JavaFX-style arrow/page/thumb input around the grid's existing scroll state.
use egui::{Color32, Id, Pos2, Rect, Sense, Stroke, Ui};
#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Idle,
    Arrow(f32),
    Page(f32),
    Thumb(f32),
}
#[derive(Clone, Copy, Default)]
pub(crate) struct State {
    mode: Mode,
    next_repeat: f64,
}
pub(crate) struct Metrics {
    pub extent: f32,
    pub total: f32,
    pub unit: f32,
}
pub(crate) fn show(
    ui: &mut Ui,
    rect: Rect,
    vertical: bool,
    offset: &mut f32,
    metrics: Metrics,
    id: Id,
    interaction: &mut State,
) {
    let Metrics {
        extent,
        total,
        unit,
    } = metrics;
    let axis = usize::from(vertical);
    let cross = 1 - axis;
    let arrow = rect.size()[cross].min(rect.size()[axis] / 2.);
    let start = rect.min[axis] + arrow;
    let end = rect.max[axis] - arrow;
    let track_len = (end - start).max(0.);
    let maximum = (total - extent).max(0.);
    *offset = (*offset).clamp(0., maximum);
    let length = if total > 0. {
        (track_len * extent / total).clamp(16f32.min(track_len), track_len)
    } else {
        track_len
    };
    let travel = (track_len - length).max(0.);
    let thumb_start = start
        + if maximum > 0. {
            *offset / maximum * travel
        } else {
            0.
        };
    let segment = |from: f32, to: f32| {
        let mut min = rect.min;
        let mut max = rect.max;
        min[axis] = from;
        max[axis] = to;
        Rect::from_min_max(min, max)
    };
    let thumb = segment(thumb_start, thumb_start + length);
    let focus = ui.memory(|memory| memory.focused());
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let (point, pressed, down, now) = ui.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.time,
        )
    });
    let before = *offset;
    if pressed
        && response.hovered()
        && let Some(point) = point
    {
        interaction.mode = if point[axis] < start {
            Mode::Arrow(-1.)
        } else if point[axis] > end {
            Mode::Arrow(1.)
        } else if thumb.contains(point) {
            Mode::Thumb(point[axis] - thumb_start)
        } else {
            Mode::Page(if point[axis] < thumb_start { -1. } else { 1. })
        };
        interaction.next_repeat = now + 0.2;
        match interaction.mode {
            Mode::Arrow(direction) => *offset += direction * unit,
            Mode::Page(direction) => *offset += direction * extent,
            _ => {}
        }
        if let Some(focus) = focus {
            ui.memory_mut(|memory| memory.request_focus(focus));
        }
    } else if down
        && response.is_pointer_button_down_on()
        && let Some(point) = point
    {
        match interaction.mode {
            Mode::Thumb(grab) if travel > 0. => {
                *offset = ((point[axis] - grab - start) / travel).clamp(0., 1.) * maximum
            }
            Mode::Arrow(direction)
                if now >= interaction.next_repeat
                    && rect.contains(point)
                    && ((direction < 0. && point[axis] < start)
                        || (direction > 0. && point[axis] > end)) =>
            {
                *offset += direction * unit;
                interaction.next_repeat = now + 0.2;
            }
            Mode::Page(direction)
                if now >= interaction.next_repeat
                    && rect.contains(point)
                    && ((direction < 0. && point[axis] < thumb_start)
                        || (direction > 0. && point[axis] > thumb_start + length)) =>
            {
                *offset += direction * extent;
                interaction.next_repeat = now + 0.2;
            }
            _ => {}
        }
    }
    if !down {
        interaction.mode = Mode::Idle;
    }
    if !matches!(interaction.mode, Mode::Idle | Mode::Thumb(_)) && down {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    *offset = (*offset).clamp(0., maximum);
    if *offset != before {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0, Color32::from_gray(241));
    painter.rect_stroke(
        rect,
        0,
        Stroke::new(1., Color32::from_gray(218)),
        egui::StrokeKind::Inside,
    );
    let active = response.hovered() || response.is_pointer_button_down_on();
    let thumb_start = start
        + if maximum > 0. {
            *offset / maximum * travel
        } else {
            0.
        };
    let thumb = segment(thumb_start, thumb_start + length).shrink(2.);
    painter.rect_filled(
        thumb,
        2.,
        Color32::from_gray(if active { 202 } else { 218 }),
    );
    painter.rect_stroke(
        thumb,
        2.,
        Stroke::new(1., Color32::from_gray(186)),
        egui::StrokeKind::Inside,
    );
    for (center, direction) in [
        (segment(rect.min[axis], start).center(), -1.),
        (segment(end, rect.max[axis]).center(), 1.),
    ] {
        let point = |along: f32, across: f32| {
            let mut point: Pos2 = center;
            point[axis] += along;
            point[cross] += across;
            point
        };
        painter.add(egui::Shape::convex_polygon(
            vec![
                point(-direction * 1.5, -2.5),
                point(direction * 1.5, 0.),
                point(-direction * 1.5, 2.5),
            ],
            Color32::from_gray(100),
            Stroke::NONE,
        ));
    }
}
