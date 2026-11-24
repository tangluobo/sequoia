//! Selectable styled logs and original single-click HTTP links.
use super::LogView;
use egui::{Color32, Ui};
struct Link {
    bytes: std::ops::Range<usize>,
    chars: std::ops::Range<usize>,
}
pub(super) fn show(ui: &mut Ui, view: &mut LogView, dependencies: bool) {
    if !view.text.is_empty() {
        ensure_bold(ui.ctx());
    }
    let id = view.editor.prepare(
        ui,
        if dependencies {
            "development-dependencies"
        } else {
            "development-log"
        },
    );
    ui.painter().rect_filled(
        ui.available_rect_before_wrap(),
        0,
        Color32::from_rgb(251, 251, 252),
    );
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    ui.spacing_mut().scroll.bar_width = 12.;
    let bold = ui
        .ctx()
        .fonts(|f| f.definitions().families.contains_key(&family()));
    egui::ScrollArea::both()
        .stick_to_bottom(!dependencies)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut links = Vec::new();
            let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _: f32| {
                links.clear();
                let mut job = egui::text::LayoutJob::default();
                let mut chars = 0;
                tomato_core::development::log::fragments(buffer.as_str(), |range, style, link| {
                    let value = &buffer.as_str()[range.clone()];
                    let count = value.chars().count();
                    let [r, g, b] = style.color;
                    let color = if link {
                        Color32::from_rgb(25, 118, 210)
                    } else {
                        Color32::from_rgb(r, g, b)
                    };
                    if link {
                        links.push(Link {
                            bytes: range,
                            chars: chars..chars + count,
                        });
                    }
                    chars += count;
                    job.append(
                        value,
                        0.,
                        egui::TextFormat {
                            font_id: egui::FontId::new(
                                12.,
                                if bold && style.bold {
                                    family()
                                } else {
                                    egui::FontFamily::Monospace
                                },
                            ),
                            color,
                            line_height: Some(16.),
                            underline: if link {
                                egui::Stroke::new(1., color)
                            } else {
                                egui::Stroke::NONE
                            },
                            ..Default::default()
                        },
                    );
                });
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let output = egui::TextEdit::multiline(&mut view.text.as_str())
                .id(id)
                .desired_width(f32::INFINITY)
                .frame(egui::Frame::NONE)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(pointer) = output.response.hover_pos()
                && (pointer.y - output.galley_pos.y) >= 0.
                && (pointer.y - output.galley_pos.y) < output.galley.size().y
            {
                let cursor = output.galley.cursor_from_pos(pointer - output.galley_pos);
                let mut index = usize::from(cursor.index);
                if pointer.x < output.galley_pos.x + output.galley.pos_from_cursor(cursor).left() {
                    index = index.saturating_sub(1);
                }
                let at = links.partition_point(|link| link.chars.end <= index);
                if let Some(link) = links.get(at).filter(|link| link.chars.contains(&index)) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if output.response.clicked()
                        && !output.response.double_clicked()
                        && !output.response.triple_clicked()
                    {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(&view.text[link.bytes.clone()]));
                    }
                }
            }
        });
    view.editor.trim(ui.ctx());
}
pub(super) fn family() -> egui::FontFamily {
    egui::FontFamily::Name("development-log-bold".into())
}
pub(super) fn ensure_bold(ctx: &egui::Context) {
    let key = egui::Id::new("development-log-bold-requested");
    if ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(key, true));
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("tomato-log-font".into())
        .spawn(move || {
            let candidates = if cfg!(windows) {
                &["C:/Windows/Fonts/consolab.ttf"][..]
            } else {
                &[
                    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
                    "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
                ][..]
            };
            let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
                return;
            };
            crate::font_assets::update(&ctx, |fonts| {
                fonts.font_data.insert(
                    "development-log-bold-data".into(),
                    egui::FontData::from_owned(bytes).into(),
                );
                let mut names = vec!["development-log-bold-data".into()];
                if fonts.font_data.contains_key("md-bold-cjk") {
                    names.push("md-bold-cjk".into());
                }
                names.extend(
                    fonts
                        .families
                        .get(&egui::FontFamily::Monospace)
                        .cloned()
                        .unwrap_or_default(),
                );
                fonts.families.insert(family(), names);
            });
            ctx.request_repaint();
        });
}
