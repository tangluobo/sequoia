//! Each Java table cell owns its right and bottom edge, never its neighbour's.
use super::*;

pub(super) fn cell(ui: &Ui, rect: Rect, fill: Color32, header: bool, selector: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0, fill);
    let right = if selector {
        Color32::from_rgb(190, 190, 188)
    } else if header {
        Color32::from_rgb(218, 218, 216)
    } else {
        Color32::from_gray(224)
    };
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.right() - 1., rect.top()), rect.max),
        0,
        right,
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.), rect.max),
        0,
        if header {
            Color32::from_gray(204)
        } else if selector {
            right
        } else {
            Color32::from_gray(224)
        },
    );
    if selector {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(1., rect.height())),
            0,
            right,
        );
    }
}
