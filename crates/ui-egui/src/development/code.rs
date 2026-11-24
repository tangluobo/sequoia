//! The shared Java editor interactions, with bounded undo and visible line decorations.
use super::Document;
use egui::{Color32, Ui, vec2};
mod markers;
pub(super) fn empty(ui: &mut Ui) {
    let area = ui.available_rect_before_wrap();
    let painter = ui.painter();
    painter.rect_filled(
        egui::Rect::from_min_size(area.min, vec2(40., 17.)),
        0,
        Color32::from_gray(248),
    );
    painter.text(
        area.min + vec2(32., 3.),
        egui::Align2::RIGHT_TOP,
        "1",
        egui::FontId::monospace(13.),
        Color32::from_gray(153),
    );
    let ruler = egui::Rect::from_min_max(egui::pos2(area.right() - 12., area.top()), area.max);
    painter.rect_filled(ruler, 0, Color32::from_rgb(243, 244, 246));
    painter.vline(
        ruler.left(),
        ruler.y_range(),
        egui::Stroke::new(1., Color32::from_rgb(215, 217, 220)),
    );
}

fn event(
    event: &egui::Event,
    text: &str,
    range: egui::text::CCursorRange,
) -> Option<egui::text_edit::EventEdit> {
    use egui::{
        Event, Key,
        text::{CCursor, CCursorRange},
        text_edit::EventEdit,
    };
    let replacement = match event {
        Event::Key {
            key: Key::D,
            pressed: true,
            modifiers,
            ..
        } if modifiers.ctrl && !modifiers.alt && !modifiers.shift => {
            return Some(
                match tomato_core::sql_edit::duplicate(
                    text,
                    range.secondary.index.into(),
                    range.primary.index.into(),
                    range.primary.index.into(),
                ) {
                    Ok(edit) => EventEdit::Replace(
                        edit.text,
                        CCursorRange::two(CCursor::new(edit.anchor), CCursor::new(edit.caret)),
                    ),
                    Err(_) => EventEdit::Selection(range),
                },
            );
        }
        Event::Key {
            key: Key::Tab,
            pressed: true,
            modifiers,
            ..
        } if !modifiers.ctrl && !modifiers.alt => {
            let (a, b) = (
                usize::from(range.secondary.index),
                usize::from(range.primary.index),
            );
            return Some(
                match tomato_core::markdown_edit::indent(text, a.min(b)..a.max(b), modifiers.shift)
                {
                    Ok(edit) => EventEdit::Replace(
                        edit.text,
                        CCursorRange::two(
                            CCursor::new(edit.selection.start),
                            CCursor::new(edit.selection.end),
                        ),
                    ),
                    Err(_) => EventEdit::Selection(range),
                },
            );
        }
        Event::Paste(value) | Event::Text(value) => value,
        _ => return None,
    };
    let (a, b) = (
        usize::from(range.secondary.index),
        usize::from(range.primary.index),
    );
    let removed = text
        .chars()
        .skip(a.min(b))
        .take(a.abs_diff(b))
        .map(char::len_utf8)
        .sum::<usize>();
    (text
        .len()
        .saturating_sub(removed)
        .saturating_add(replacement.len())
        > 8 * 1024 * 1024)
        .then_some(EventEdit::Selection(range))
}

pub(super) fn show(ui: &mut Ui, document: &mut Document, reading: bool) {
    super::log::ensure_bold(ui.ctx());
    let bold = ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&super::log::family()));
    let id = document.editor.prepare(ui, "development-code");
    let extension = document
        .path
        .extension()
        .unwrap_or_default()
        .to_string_lossy();
    let area = ui.available_rect_before_wrap();
    let text_rect = egui::Rect::from_min_max(area.min + vec2(40., 0.), area.max - vec2(12., 0.));
    let changes = &document.changes;
    let mut target = document.target_line.take();
    let line_count = document.text.split('\n').count().max(1);
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        ui.spacing_mut().scroll.bar_width = 12.;
        egui::ScrollArea::both()
            .id_salt("development-editor-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(3.);
                let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _width: f32| {
                    let mut job = egui::text::LayoutJob::default();
                    for token in tomato_core::syntax::tokens(buffer.as_str(), &extension) {
                        let [r, g, b] = token.kind.rgb();
                        let color = Color32::from_rgb(r, g, b);
                        job.append(
                            &buffer.as_str()[token.range],
                            0.,
                            egui::TextFormat {
                                font_id: if bold && token.kind == tomato_core::syntax::Kind::Keyword
                                {
                                    egui::FontId::new(13., super::log::family())
                                } else {
                                    egui::FontId::monospace(13.)
                                },
                                italics: token.kind == tomato_core::syntax::Kind::Comment,
                                line_height: Some(17.),
                                color,
                                ..Default::default()
                            },
                        );
                    }
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let mut event_editor = event;
                let mut show = |ui: &mut Ui, buffer: &mut dyn egui::TextBuffer| {
                    egui::TextEdit::multiline(buffer)
                        .id(id)
                        .font(egui::FontId::monospace(13.))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .event_editor(&mut event_editor, 32 * 1024 * 1024)
                        .show(ui)
                };
                let background_index = ui.painter().add(egui::Shape::Noop);
                let output = if document.read_only || reading {
                    show(ui, &mut document.text.as_str())
                } else {
                    show(ui, &mut document.text)
                };
                let gutter =
                    egui::Rect::from_min_max(area.min, egui::pos2(text_rect.left(), area.bottom()));
                let painter = ui
                    .ctx()
                    .layer_painter(ui.layer_id())
                    .with_clip_rect(gutter.intersect(area));
                let mut backgrounds = Vec::new();
                for (index, row) in output.galley.rows.iter().enumerate() {
                    let y = output.galley_pos.y + row.pos.y;
                    let height = row.size.y;
                    if let Some(line) = target
                        && index + 1 == line
                    {
                        ui.scroll_to_rect(
                            egui::Rect::from_min_size(
                                egui::pos2(text_rect.left(), y),
                                vec2(1., height),
                            ),
                            Some(egui::Align::Center),
                        );
                        target = None;
                    }
                    if y + height < area.top() || y > area.bottom() {
                        continue;
                    }
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(gutter.left(), y - 3.),
                            egui::pos2(gutter.right(), y + height - 3.),
                        ),
                        0,
                        Color32::from_gray(248),
                    );
                    painter.text(
                        egui::pos2(gutter.right() - 8., y),
                        egui::Align2::RIGHT_TOP,
                        (index + 1).to_string(),
                        egui::FontId::monospace(13.),
                        Color32::from_gray(153),
                    );
                    if let Some(kind) = markers::at_line(changes, index + 1) {
                        backgrounds.push(egui::Shape::rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(text_rect.left(), y - 3.),
                                egui::pos2(text_rect.right(), y + height - 3.),
                            ),
                            0,
                            markers::background(kind),
                        ));
                    }
                }
                ui.painter()
                    .set(background_index, egui::Shape::Vec(backgrounds));
            });
    });
    let ruler = egui::Rect::from_min_max(egui::pos2(text_rect.right(), area.top()), area.max);
    if let Some(line) = markers::show(ui, ruler, id, changes, line_count) {
        target = Some(line);
        ui.ctx().memory_mut(|m| m.request_focus(id));
    }
    document.target_line = target;
    document.editor.trim(ui.ctx());
}
