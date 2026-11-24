//! Measured original editor/result divider, gutter and change-marker strip.
use egui::*;

pub(super) fn split(ui: &mut Ui, body: Rect, fraction: &mut Option<f32>, id: Id) -> (Rect, Rect) {
    let fraction = fraction.get_or_insert(0.6);
    if !fraction.is_finite() {
        *fraction = 0.6;
    }
    let thickness = body.height().clamp(0., 1.);
    let height = (body.height() - thickness).max(0.);
    let scale = ui.ctx().pixels_per_point();
    let round = |position: f32| (position * scale).round() / scale;
    let mut position = round(body.top() + height * fraction.clamp(0., 1.));
    let handle = Rect::from_min_size(pos2(body.left(), position), vec2(body.width(), thickness));
    let response = ui
        .interact(
            handle,
            id.with("sql-editor-result-divider"),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeVertical);
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *fraction = ((pointer.y - body.top()) / height.max(1.)).clamp(0., 1.);
        position = round(body.top() + height * *fraction);
    }
    let divider = Rect::from_min_size(pos2(body.left(), position), vec2(body.width(), thickness));
    ui.painter()
        .rect_filled(divider, 0., Color32::from_gray(229));
    (
        Rect::from_min_max(body.min, divider.right_top()),
        Rect::from_min_max(divider.left_bottom(), body.max),
    )
}

pub(super) fn editor(
    ui: &mut Ui,
    rect: Rect,
    text: &mut String,
    state: &mut crate::tabs::completion::State,
    tab: &str,
    run_enabled: bool,
) {
    ui.painter().rect_filled(rect, 0., Color32::WHITE);
    let marker = Rect::from_min_max(
        pos2((rect.right() - 12.).max(rect.left()), rect.top()),
        rect.max,
    );
    ui.painter()
        .rect_filled(marker, 0., Color32::from_rgb(243, 244, 246));
    ui.painter().line_segment(
        [
            marker.left_top() + vec2(0.5, 0.),
            marker.left_bottom() + vec2(0.5, 0.),
        ],
        Stroke::new(1., Color32::from_rgb(215, 217, 220)),
    );
    let content = Rect::from_min_max(rect.min, marker.left_bottom());
    if content.width() <= 0. || content.height() <= 0. {
        return;
    }
    let scroll = ui
        .scope_builder(
            UiBuilder::new()
                .id_salt((tab, "sql-editor-area"))
                .max_rect(content),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(content));
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                ui.spacing_mut().scroll.bar_width = 12.;
                ScrollArea::both()
                    .id_salt((tab, "sql-code-scroll"))
                    .max_height(content.height())
                    .min_scrolled_height(content.height())
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| {
                        super::completion::render_in(
                            ui,
                            text,
                            state,
                            tab,
                            run_enabled,
                            viewport.size(),
                        );
                    })
            },
        )
        .inner;
    if let Some(galley) = &state.galley {
        let gutter = Rect::from_min_max(
            scroll.inner_rect.min,
            pos2(
                (scroll.inner_rect.left() + 40.).min(scroll.inner_rect.right()),
                scroll.inner_rect.bottom(),
            ),
        );
        let painter = ui.painter_at(gutter);
        let top = state.galley_pos.y;
        let end = (top + galley.size().y).min(gutter.bottom());
        if end > gutter.top() {
            painter.rect_filled(
                Rect::from_min_max(gutter.min, pos2(gutter.right(), end)),
                0.,
                Color32::from_gray(248),
            );
        }
        let start = galley
            .rows
            .partition_point(|row| top + row.pos.y + row.size.y < gutter.top());
        for (index, row) in galley.rows.iter().enumerate().skip(start) {
            let y = top + row.pos.y;
            if y > gutter.bottom() {
                break;
            }
            // Original LineNumberFactory reserves its invisible graphic spacer.
            painter.text(
                pos2(gutter.left() + 20., y),
                Align2::RIGHT_TOP,
                (index + 1).to_string(),
                FontId::monospace(13.),
                Color32::from_gray(136),
            );
        }
    }
}
