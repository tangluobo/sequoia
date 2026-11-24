//! Reuse live editor galleys; the cache never owns a second SQL document.
use egui::{Color32, FontFamily, FontId, Galley, TextFormat, Ui, text::LayoutJob};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};
#[derive(Clone, Default)]
struct Cache {
    entries: Vec<(f32, u64, Weak<Galley>)>,
}
pub(super) fn bold_font(ctx: &egui::Context) {
    let start = ctx.data_mut(|data| {
        let started = data.get_temp_mut_or_default::<bool>(egui::Id::new("sql-bold-requested"));
        if *started {
            false
        } else {
            *started = true;
            true
        }
    });
    if !start {
        return;
    }
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("tomato-sql-font".into())
        .spawn(move || {
            let bytes = [
                "C:/Windows/Fonts/consolab.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
            ]
            .into_iter()
            .find_map(|path| std::fs::read(path).ok());
            let Some(bytes) = bytes else { return };
            crate::font_assets::update(&ctx, |fonts| {
                fonts.font_data.insert(
                    "sql-bold-data".into(),
                    egui::FontData::from_owned(bytes).into(),
                );
                let mut fallback = fonts.families[&FontFamily::Monospace].clone();
                fallback.insert(0, "sql-bold-data".into());
                fonts
                    .families
                    .insert(FontFamily::Name("sql-bold".into()), fallback);
            });
            ctx.request_repaint();
        });
}
pub(super) fn layout(ui: &Ui, buffer: &dyn egui::TextBuffer, _width: f32) -> Arc<Galley> {
    use tomato_core::syntax::Kind;
    bold_font(ui.ctx());
    let text = buffer.as_str();
    let ppp = ui.ctx().pixels_per_point();
    let bold = ui
        .ctx()
        .fonts(|fonts| fonts.definitions().font_data.contains_key("sql-bold-data"));
    let font_key = ui.ctx().fonts(|fonts| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for (name, data) in &fonts.definitions().font_data {
            name.hash(&mut hash);
            (Arc::as_ptr(data) as usize).hash(&mut hash);
        }
        for (family, names) in &fonts.definitions().families {
            family.hash(&mut hash);
            names.hash(&mut hash);
        }
        hash.finish()
    });
    let cache_id = egui::Id::new("sql-highlight-galleys");
    if let Some(galley) = ui.ctx().data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id);
        cache
            .entries
            .retain(|(_, _, galley)| galley.strong_count() > 0);
        cache
            .entries
            .iter()
            .rev()
            .filter(|(scale, font, _)| *scale == ppp && *font == font_key)
            .filter_map(|(_, _, galley)| galley.upgrade())
            .find(|galley| galley.job.text == text)
    }) {
        return galley;
    }
    let plain = TextFormat {
        line_height: Some(17.),
        font_id: FontId::monospace(13.),
        color: Color32::from_rgb(8, 8, 8),
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    // An empty editor still needs the first line's font metrics for its caret.
    if text.is_empty() || text.len() > 256 * 1024 {
        job.append(text, 0., plain.clone());
    } else {
        let mut end = 0;
        for token in tomato_core::syntax::tokens(text, "sql")
            .into_iter()
            .take(16_384)
        {
            let rgb = token.kind.rgb();
            let mut format = plain.clone();
            format.color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            format.italics = token.kind == Kind::Comment;
            if bold && token.kind == Kind::Keyword {
                format.font_id = FontId::new(13., FontFamily::Name("sql-bold".into()));
            }
            end = token.range.end;
            job.append(&text[token.range], 0., format);
        }
        if end < text.len() {
            job.append(&text[end..], 0., plain);
        }
    }
    job.wrap.max_width = f32::INFINITY;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.ctx().data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id);
        if cache.entries.len() >= 8 {
            cache.entries.remove(0);
        }
        cache.entries.push((ppp, font_key, Arc::downgrade(&galley)));
    });
    galley
}
