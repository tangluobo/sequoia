//! Source styling follows markdown-editor-highlight.css; egui caches the galley.
pub(in crate::panels) use crate::font_assets::bold::ensure as ensure_bold_cjk;
pub(super) fn code(source: &str, language: &str, size: f32) -> egui::text::LayoutJob {
    use tomato_core::syntax::Kind;
    let mut job = egui::text::LayoutJob {
        align_on_baseline: true,
        ..Default::default()
    };
    for token in tomato_core::syntax::tokens(source, language) {
        let rgb = token.kind.rgb();
        job.append(
            &source[token.range],
            0.,
            egui::TextFormat {
                font_id: egui::FontId::monospace(size),
                color: egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                italics: token.kind == Kind::Comment,
                ..Default::default()
            },
        );
    }
    job
}
pub(super) fn layout(
    ui: &egui::Ui,
    buffer: &dyn egui::TextBuffer,
    _width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob {
        align_on_baseline: true,
        ..Default::default()
    };
    let mut fence: Option<String> = None;
    for line in buffer.as_str().split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if !marker && let Some(language) = &fence {
            let highlighted = code(line, language, 13.);
            for section in highlighted.sections {
                let start: usize = section.byte_range.start.into();
                let end: usize = section.byte_range.end.into();
                job.append(&line[start..end], 0., section.format);
            }
            continue;
        }
        let color = if marker {
            egui::Color32::from_rgb(128, 128, 128)
        } else if trimmed.starts_with('#') {
            egui::Color32::from_rgb(17, 99, 166)
        } else if trimmed.starts_with('>') {
            egui::Color32::from_rgb(26, 115, 232)
        } else {
            egui::Color32::from_gray(51)
        };
        job.append(
            line,
            0.,
            egui::TextFormat {
                font_id: egui::FontId::monospace(13.),
                color,
                ..Default::default()
            },
        );
        if marker {
            fence = if fence.is_some() {
                None
            } else {
                Some(trimmed.trim_start_matches(['`', '~']).trim().to_owned())
            };
        }
    }
    // Java's editor keeps source lines intact and scrolls horizontally.
    job.wrap.max_width = f32::INFINITY;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub(super) fn line_number(
    ui: &egui::Ui,
    number: usize,
    right_top: egui::Pos2,
    baseline: Option<f32>,
    size: f32,
    color: egui::Color32,
) {
    let galley =
        ui.painter()
            .layout_no_wrap(number.to_string(), egui::FontId::monospace(size), color);
    let own_baseline = galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(0., |glyph| glyph.pos.y);
    ui.painter().galley(
        right_top + egui::vec2(-galley.size().x, baseline.map_or(0., |b| b - own_baseline)),
        galley,
        color,
    );
}
