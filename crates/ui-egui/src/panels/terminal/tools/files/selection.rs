//! File-browser rubber band, in content coordinates so scrolling preserves its anchor.
use crate::tabs::ssh::Pane;
use egui::{Color32, Rect, Stroke, StrokeKind, Ui};

pub(super) fn update(
    ui: &Ui,
    pane: &mut Pane,
    mode: &str,
    bounds: Rect,
    indices: &[usize],
    rect_for: impl Fn(usize) -> Rect,
) -> (Option<Rect>, egui::Response) {
    let response = ui.interact(
        bounds,
        ui.make_persistent_id("file-range-selection"),
        egui::Sense::click_and_drag(),
    );
    let range = range(ui, pane, mode, bounds, indices, rect_for, true);
    (range, response)
}
fn range(
    ui: &Ui,
    pane: &mut Pane,
    mode: &str,
    bounds: Rect,
    indices: &[usize],
    rect_for: impl Fn(usize) -> Rect,
    auto_scroll: bool,
) -> Option<Rect> {
    if !ui.is_enabled()
        || pane.rename.as_ref().is_some_and(|edit| edit.due.is_none())
        || egui::Popup::is_any_open(ui.ctx())
        || pane.file_drag.as_ref().is_some_and(|drag| {
            drag.context.0 != pane.path || drag.context.1 != pane.filter || drag.context.2 != mode
        })
        || ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused)
    {
        pane.file_drag = None;
        return None;
    }
    let focus = ui.make_persistent_id("file-range-selection");
    let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else {
        pane.file_drag = None;
        return None;
    };
    let visible = bounds.intersect(ui.clip_rect());
    let additive = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift || i.modifiers.mac_cmd);
    if ui.input(|i| i.pointer.primary_pressed())
        && visible.contains(pointer)
        && ui.rect_contains_pointer(visible)
        && !indices
            .iter()
            .enumerate()
            .any(|(ordinal, _)| rect_for(ordinal).contains(pointer))
    {
        pane.file_drag = Some(crate::tabs::ssh::FileDrag {
            start: pointer - bounds.min,
            context: (pane.path.clone(), pane.filter.clone(), mode.to_owned()),
        });
        pane.column_selected = None;
        ui.memory_mut(|m| m.request_focus(focus));
        pane.focus = Some(focus);
        if !additive {
            pane.selected.clear();
            pane.anchor = None;
        }
    }
    let drag = pane.file_drag.as_ref()?;
    let end = pointer.clamp(bounds.min, bounds.max);
    let selection = Rect::from_two_pos(bounds.min + drag.start, end);
    if !additive {
        pane.selected.clear();
    }
    if selection.width() > 0. && selection.height() > 0. {
        for (ordinal, index) in indices.iter().enumerate() {
            let item = rect_for(ordinal);
            if item.left() < selection.right()
                && item.right() > selection.left()
                && item.top() < selection.bottom()
                && item.bottom() > selection.top()
            {
                pane.selected.insert(*index);
            }
        }
    }
    pane.anchor = pane.selected.first().copied();
    if !ui.input(|i| i.pointer.primary_down()) {
        pane.file_drag = None;
        return None;
    }
    // Match the original 25 px edge / 8 px per animation tick; no idle timer.
    let speed = |p: f32, min: f32, max: f32| {
        if p < min + 25. {
            ((min + 25. - p) / 25.).clamp(0., 1.) * 8.
        } else if p > max - 25. {
            -((p - max + 25.) / 25.).clamp(0., 1.) * 8.
        } else {
            0.
        }
    };
    let delta = egui::vec2(
        speed(pointer.x, visible.left(), visible.right()),
        speed(pointer.y, visible.top(), visible.bottom()),
    );
    if auto_scroll && delta != egui::Vec2::ZERO {
        ui.scroll_with_delta_animation(delta, egui::style::ScrollAnimation::none());
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    Some(selection)
}

pub(super) fn paint(ui: &Ui, selection: Option<Rect>, bounds: Rect) {
    if let Some(rect) = selection {
        ui.painter()
            .with_clip_rect(bounds.intersect(ui.clip_rect()))
            .rect(
                rect,
                0,
                Color32::from_rgba_unmultiplied(51, 153, 255, 38),
                Stroke::new(1., Color32::from_rgba_unmultiplied(51, 153, 255, 204)),
                StrokeKind::Inside,
            );
    }
}

pub(super) fn update_in_column(
    ui: &Ui,
    pane: &mut Pane,
    bounds: Rect,
    indices: &[usize],
    rect_for: impl Fn(usize) -> Rect,
) -> (Option<Rect>, egui::Response) {
    let response = ui.interact(
        bounds,
        ui.make_persistent_id("file-range-selection"),
        egui::Sense::click_and_drag(),
    );
    (
        range(ui, pane, "COLUMN", bounds, indices, rect_for, false),
        response,
    )
}
