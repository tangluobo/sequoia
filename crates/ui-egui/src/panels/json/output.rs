use super::*;
use crate::tabs::json::{
    JsonView,
    selection::{Point, Selection},
};
use egui::{Align2, FontId, Key, Pos2, Rect, Sense};
mod gutter;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Rich,
    Raw,
}
mod input;
mod metrics;
#[cfg(test)]
mod tests;

pub(super) fn render(ui: &mut Ui, tool: &mut ToolState) {
    if viewer(
        ui,
        &tool.output,
        &mut tool.json_view,
        tool.json_tree,
        &mut tool.json_reset_output_scroll,
    ) {
        tool.json_status.success("已复制节点JSON到剪贴板！");
    }
}
pub(crate) fn viewer(
    ui: &mut Ui,
    text: &str,
    view: &mut JsonView,
    tree: bool,
    reset: &mut bool,
) -> bool {
    if view.metrics_scale != ui.ctx().pixels_per_point() {
        view.long_lines.clear();
        view.metrics_scale = ui.ctx().pixels_per_point();
    }
    let mut selection = std::mem::take(&mut view.selection);
    let kind = if tree { Kind::Rich } else { Kind::Raw };
    let syntax = tree && view.is_structured();
    let mut toggle = None;
    let mut copy = None;
    let font = FontId::monospace(if tree { 12. } else { 13. });
    // Actual Java paragraph spacing at 96 DPI: RichTextFX 14px, TextArea 19px.
    let height = if tree { 14. } else { 19. };
    let id = ui.id().with("json-readonly-text");
    let gutter = if tree { 70. } else { 5. };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.;
        let mut scroll = ScrollArea::both()
            .id_salt("json-output-scroll")
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..Default::default()
            });
        if std::mem::take(reset) {
            scroll = scroll.scroll_offset(egui::Vec2::ZERO);
        }
        scroll.show_rows(ui, height, view.visible.len(), |ui, rows| {
            let origin = ui.cursor().min - egui::vec2(0., rows.start as f32 * height);
            let clip = ui.clip_rect();
            let viewport =
                Rect::from_min_max(egui::pos2(clip.left() + gutter, clip.top()), clip.max);
            let response = ui
                .interact(viewport, id, Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::Text);
            view.viewport = Some(crate::tabs::json::Viewport {
                rect: viewport,
                origin,
                line_height: height,
                gutter,
            });
            input::handle(
                ui,
                text,
                view,
                &mut selection,
                &font,
                height,
                origin,
                gutter,
                &response,
                kind,
            );
            for row in rows {
                let line = view.visible[row];
                let display = view.visible_line(text, row);
                let data = display.as_ref();
                let measured = metrics::layout(ui, data, syntax, view, &font, line);
                let width = (measured.width + 8.).max(ui.available_width() - gutter);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(gutter + width, height), Sense::hover());
                let left = rect.left() + gutter;
                let painter = ui.painter().with_clip_rect(viewport);
                if tree {
                    let rect = Rect::from_min_size(
                        egui::pos2(clip.left(), rect.top()),
                        egui::vec2(gutter, height),
                    );
                    match gutter::show(
                        ui,
                        id,
                        rect,
                        row,
                        view.node(line).map(|_| (line, view.folded(line))),
                    ) {
                        Some(gutter::Action::Toggle(line)) => toggle = Some(line),
                        Some(gutter::Action::Copy(line)) => copy = Some(line),
                        None => {}
                    }
                }
                if let Some(range) = selection.row_range(row, data.len()) {
                    let start = metrics::x(ui, data, view, &font, line, range.start);
                    let end = metrics::x(ui, data, view, &font, line, range.end);
                    let (_, last) = selection.range().unwrap();
                    let end = if row < last.row {
                        if tree {
                            (viewport.right() - left).max(end)
                        } else {
                            end + ui.fonts_mut(|f| f.glyph_width(&font, ' '))
                        }
                    } else {
                        end
                    };
                    painter.rect_filled(
                        Rect::from_min_max(
                            egui::pos2(left + start, rect.top()),
                            egui::pos2(left + end, rect.bottom()),
                        ),
                        0.,
                        if tree {
                            Color32::from_rgb(30, 144, 255)
                        } else if response.has_focus() {
                            Color32::from_rgb(0, 150, 201)
                        } else {
                            Color32::from_gray(211)
                        },
                    );
                }
                let (galley, x) = if let Some(galley) = measured.galley {
                    (galley, 0.)
                } else {
                    let measured = &view.long_lines[&line];
                    let (window, x, prefix) = measured.window(
                        data,
                        f64::from((clip.left() - left).max(0.)),
                        f64::from(clip.width()),
                    );
                    (
                        ui.painter().layout_job(if syntax {
                            highlight_from(window, prefix)
                        } else {
                            egui::text::LayoutJob::simple(
                                window.into(),
                                font.clone(),
                                Color32::from_gray(51),
                                f32::INFINITY,
                            )
                        }),
                        x as f32,
                    )
                };
                let y = rect.center().y - galley.size().y / 2.;
                painter.galley(egui::pos2(left + x, y), galley, Color32::BLACK);
                if response.has_focus() && selection.range().is_none() && selection.head.row == row
                {
                    let elapsed = ui.input(|i| i.time) - selection.blink_start;
                    if elapsed.rem_euclid(1.) < 0.5 {
                        let x = left + metrics::x(ui, data, view, &font, line, selection.head.byte);
                        painter.vline(x, rect.y_range(), egui::Stroke::new(1., Color32::BLACK));
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
        });
    });
    view.selection = selection;
    if let Some(line) = toggle {
        view.toggle(line);
    }
    if let Some(value) = copy.and_then(|line| view.copy(text, line)) {
        ui.ctx().copy_text(value);
        return true;
    }
    false
}
