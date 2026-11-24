//! Compact projection of Java's per-line markers. Storage follows viewport pixels.
use egui::{Color32, Rect, Ui, pos2};
use tomato_core::development::{ChangeKind, LineChange};
pub(super) fn background(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => Color32::from_rgb(228, 242, 223),
        ChangeKind::Modified => Color32::from_rgb(220, 236, 255),
        ChangeKind::Deleted => Color32::from_rgb(247, 217, 220),
    }
}
fn color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => Color32::from_rgb(98, 168, 82),
        ChangeKind::Modified => Color32::from_rgb(53, 116, 185),
        ChangeKind::Deleted => Color32::from_rgb(199, 84, 80),
    }
}
pub(super) fn at_line(changes: &[LineChange], line: usize) -> Option<ChangeKind> {
    // Git emits hunks in line order; a later insertion can replace a deletion marker.
    let index = changes
        .partition_point(|c| c.start <= line)
        .checked_sub(1)?;
    changes.get(index).filter(|c| line <= c.end).map(|c| c.kind)
}
pub(super) fn show(
    ui: &mut Ui,
    ruler: Rect,
    id: egui::Id,
    changes: &[LineChange],
    line_count: usize,
) -> Option<usize> {
    let painter = ui.painter().with_clip_rect(ruler);
    painter.rect_filled(ruler, 0, Color32::from_rgb(243, 244, 246));
    painter.vline(
        ruler.left(),
        ruler.y_range(),
        egui::Stroke::new(1., Color32::from_rgb(215, 217, 220)),
    );
    if changes.is_empty() {
        return None;
    }
    let count = line_count.max(1);
    let height = ruler.height().ceil().clamp(1., 16384.) as usize;
    let scale = ruler.height() / height as f32;
    let marker_height = (ruler.height() / count as f32).clamp(4., 10.);
    let travel = (ruler.height() - marker_height).max(0.);
    let step = travel / count.saturating_sub(1).max(1) as f32;
    let mut pixels = vec![None; height];
    for change in changes {
        let first = change.start.clamp(1, count);
        let last = change.end.clamp(first, count);
        let mut paint = |a: usize, b: usize| {
            let top = (a - 1) as f32 * step;
            let bottom = (b - 1) as f32 * step + marker_height;
            let start = (top / scale).floor() as usize;
            let end = ((bottom / scale).ceil() as usize).min(height);
            if start < end {
                pixels[start..end].fill(Some(change.kind));
            }
        };
        if step <= marker_height {
            paint(first, last);
        } else {
            for line in first..=last {
                paint(line, line);
            }
        }
    }
    let mut start = 0;
    while start < height {
        let kind = pixels[start];
        let mut end = start + 1;
        while end < height && pixels[end] == kind {
            end += 1;
        }
        if let Some(kind) = kind {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(ruler.left() + 2., ruler.top() + start as f32 * scale),
                    pos2(ruler.left() + 11., ruler.top() + end as f32 * scale),
                ),
                1,
                color(kind),
            );
        }
        start = end;
    }
    let response = ui
        .interact(ruler, id.with("git-change-ruler"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let pos = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())?;
    let line = 1.
        + ((pos.y - ruler.top()) / ruler.height().max(1.)).clamp(0., 1.)
            * count.saturating_sub(1) as f32;
    let nearest = changes
        .iter()
        .map(|c| {
            let candidate =
                (line.round() as usize).clamp(c.start.max(1), c.end.max(c.start).max(1));
            (candidate, (candidate as f32 - line).abs(), c.kind)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let clicked = response.clicked();
    let row = ((pos.y - ruler.top()) / scale).floor() as usize;
    if pixels.get(row).is_some_and(Option::is_some) {
        response.on_hover_text(format!(
            "第 {} 行：{}",
            nearest.0,
            match nearest.2 {
                ChangeKind::Added => "新增",
                ChangeKind::Modified => "修改",
                ChangeKind::Deleted => "删除",
            }
        ));
    }
    clicked.then_some(nearest.0.min(count))
}
