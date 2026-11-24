use super::*;
pub(super) struct Layout {
    pub width: f32,
    pub galley: Option<std::sync::Arc<egui::Galley>>,
}
pub(super) fn layout(
    ui: &Ui,
    text: &str,
    tree: bool,
    view: &mut JsonView,
    font: &FontId,
    line: u32,
) -> Layout {
    if text.len() > 4096 {
        ensure(ui, text, view, font, line);
        Layout {
            width: view.long_lines[&line].width as f32,
            galley: None,
        }
    } else {
        let galley = ui.painter().layout_job(if tree {
            highlight(text)
        } else {
            egui::text::LayoutJob::simple(
                text.into(),
                font.clone(),
                Color32::from_gray(51),
                f32::INFINITY,
            )
        });
        Layout {
            width: galley.size().x,
            galley: Some(galley),
        }
    }
}
fn ensure(ui: &Ui, text: &str, view: &mut JsonView, font: &FontId, line: u32) {
    if !view.long_lines.contains_key(&line) {
        if view.long_lines.len() >= 128
            && let Some(old) = view
                .long_lines
                .keys()
                .max_by_key(|old| old.abs_diff(line))
                .copied()
        {
            view.long_lines.remove(&old);
        }
        view.long_lines.insert(
            line,
            crate::tabs::json::long_line::LongLine::new(text, |ch| {
                ui.fonts_mut(|f| f.glyph_width(font, ch))
            }),
        );
    }
}
pub(super) fn byte(
    ui: &Ui,
    text: &str,
    view: &mut JsonView,
    font: &FontId,
    line: u32,
    x: f32,
) -> usize {
    if text.len() > 4096 {
        ensure(ui, text, view, font, line);
        view.long_lines[&line].byte_at_x(text, f64::from(x), |ch| {
            ui.fonts_mut(|f| f.glyph_width(font, ch))
        })
    } else {
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.into(), font.clone(), Color32::BLACK));
        let character = usize::from(
            galley
                .cursor_from_pos(egui::vec2(x, galley.size().y / 2.))
                .index,
        );
        crate::tabs::json::selection::floor(
            text,
            text.char_indices()
                .nth(character)
                .map_or(text.len(), |(byte, _)| byte),
        )
    }
}
pub(super) fn x(
    ui: &Ui,
    text: &str,
    view: &mut JsonView,
    font: &FontId,
    line: u32,
    byte: usize,
) -> f32 {
    if text.len() > 4096 {
        ensure(ui, text, view, font, line);
        view.long_lines[&line].x_at_byte(text, byte, |ch| ui.fonts_mut(|f| f.glyph_width(font, ch)))
            as f32
    } else {
        let byte = crate::tabs::json::selection::floor(text, byte);
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.into(), font.clone(), Color32::BLACK));
        galley
            .pos_from_cursor(egui::text::CCursor::new(text[..byte].chars().count()))
            .left()
    }
}
