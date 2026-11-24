//! Content-sized grid matching the Java preview's cell padding and shared edges.
use super::{Images, Inline, Span, Visits, paint_inline, text_layout};
use crate::tabs::markdown::TableGeometry;
use egui::{Align, Color32, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

const COLUMN_PADDING: f32 = 21.;
const ROW_PADDING: f32 = 13.;
const EDGE: Color32 = Color32::from_rgb(223, 226, 229);

pub(super) fn render<'a>(
    ui: &mut Ui,
    rows: &'a [Vec<Vec<Span>>],
    images: Images<'a>,
    visits: &mut Visits<'a>,
    cache: Option<&mut TableGeometry>,
) {
    let mut temporary = TableGeometry::default();
    let geometry = cache.unwrap_or(&mut temporary);
    let available = (ui.available_width() - 1.).max(1.);
    let scale = ui.ctx().pixels_per_point();
    let bold_cjk = ui.fonts(|fonts| fonts.definitions().font_data.contains_key("md-bold-cjk"));
    if geometry.row_ends.is_empty()
        || (geometry.available - available).abs() > 0.5
        || geometry.scale != scale
        || geometry.bold_cjk != bold_cjk
    {
        measure(ui, rows, images, available, geometry);
        geometry.scale = scale;
        geometry.bold_cjk = bold_cjk;
    }
    let width = geometry.widths.iter().sum::<f32>() + 1.;
    if visits.interactive && width > available + 2. {
        ScrollArea::horizontal().show(ui, |ui| paint(ui, rows, images, visits, geometry, width));
    } else {
        paint(ui, rows, images, visits, geometry, width);
    }
}

fn measure(
    ui: &Ui,
    rows: &[Vec<Vec<Span>>],
    images: Images<'_>,
    available: f32,
    geometry: &mut TableGeometry,
) {
    let columns = rows.first().map_or(0, Vec::len);
    let minimum = if images.interactive {
        COLUMN_PADDING
    } else {
        (available / columns.max(1) as f32).min(COLUMN_PADDING)
    };
    geometry.widths.clear();
    geometry.widths.resize(columns, minimum);
    for (index, row) in rows.iter().enumerate() {
        for (width, spans) in geometry.widths.iter_mut().zip(row) {
            let natural = text_layout(ui, spans, 13., index == 0, f32::INFINITY, images);
            *width = width.max(natural.galley.size().x.ceil() + COLUMN_PADDING);
        }
    }
    shrink_columns(&mut geometry.widths, available, minimum);
    geometry.row_ends.clear();
    let mut y = 1.;
    for (index, row) in rows.iter().enumerate() {
        let height = row
            .iter()
            .zip(&geometry.widths)
            .map(|(spans, width)| {
                cell(ui, spans, *width, index == 0, images)
                    .galley
                    .size()
                    .y
                    .ceil()
            })
            .fold(17., f32::max);
        y += height + ROW_PADDING;
        geometry.row_ends.push(y);
    }
    geometry.available = available;
}

fn shrink_columns(widths: &mut [f32], available: f32, minimum: f32) {
    if widths.iter().sum::<f32>() <= available {
        return;
    }
    // GridPane removes the same amount from each column until it reaches its minimum.
    let mut low = 0.;
    let mut high = widths.iter().copied().fold(minimum, f32::max) - minimum;
    for _ in 0..20 {
        let reduction = (low + high) / 2.;
        if widths
            .iter()
            .map(|width| (width - reduction).max(minimum))
            .sum::<f32>()
            > available
        {
            low = reduction;
        } else {
            high = reduction;
        }
    }
    for width in widths {
        *width = (*width - high).max(minimum);
    }
}

fn cell<'a>(ui: &Ui, spans: &'a [Span], width: f32, bold: bool, images: Images<'a>) -> Inline<'a> {
    text_layout(
        ui,
        spans,
        13.,
        bold,
        (width - COLUMN_PADDING).max(1.),
        images,
    )
}

fn paint<'a>(
    ui: &mut Ui,
    rows: &'a [Vec<Vec<Span>>],
    images: Images<'a>,
    visits: &mut Visits<'a>,
    geometry: &TableGeometry,
    width: f32,
) {
    let Some(&height) = geometry.row_ends.last() else {
        return;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let clip = rect.intersect(ui.clip_rect());
    if !clip.is_positive() {
        return;
    }
    let top = geometry
        .row_ends
        .partition_point(|end| *end <= clip.top() - rect.top());
    for (index, row) in rows.iter().enumerate().skip(top) {
        let y = if index == 0 {
            1.
        } else {
            geometry.row_ends[index - 1]
        };
        let y = rect.top() + y;
        if y >= clip.bottom() {
            break;
        }
        let bottom = rect.top() + geometry.row_ends[index];
        let mut x = rect.left();
        for (column, (spans, &width)) in row.iter().zip(&geometry.widths).enumerate() {
            let bounds = Rect::from_min_max(pos2(x, y), pos2(x + width, bottom));
            x += width;
            if !bounds.intersects(clip) {
                continue;
            }
            let painter = ui.painter();
            painter.rect_filled(
                bounds,
                0.,
                if index == 0 {
                    Color32::from_rgb(246, 248, 250)
                } else {
                    Color32::WHITE
                },
            );
            painter.rect_filled(
                Rect::from_min_size(bounds.min, vec2(1., bounds.height())),
                0.,
                EDGE,
            );
            painter.rect_filled(
                Rect::from_min_size(
                    bounds.left_bottom() - vec2(0., 1.),
                    vec2(bounds.width(), 1.),
                ),
                0.,
                EDGE,
            );
            let content = Rect::from_min_size(
                bounds.min + vec2(11., 6.),
                vec2(
                    (width - COLUMN_PADDING).max(1.),
                    bounds.height() - ROW_PADDING,
                ),
            );
            let layout = cell(ui, spans, width, index == 0, images);
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt((index, column))
                    .max_rect(content)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_clip_rect(clip.intersect(bounds));
                    paint_inline(ui, layout, visits);
                },
            );
        }
    }
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 1.)),
        0.,
        EDGE,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.right_top() - vec2(1., 0.), vec2(1., rect.height())),
        0.,
        EDGE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_keeps_cell_minimum_and_removes_equal_widths() {
        let mut widths = [110., 590., 45.];
        shrink_columns(&mut widths, 558., 21.);
        assert!((widths.iter().sum::<f32>() - 558.).abs() < 0.01);
        assert_eq!(widths[2], 21.);
        assert!(((590. - widths[1]) - (110. - widths[0])).abs() < 0.01);
    }
}
