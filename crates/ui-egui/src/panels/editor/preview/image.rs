use super::*;
use crate::tabs::markdown::ReadyPicture;
use tomato_services::markdown::Image;

pub(super) struct Run<'a> {
    pub index: usize,
    pub metrics: egui::epaint::text::InlineObject,
    image: &'a Image,
    visual: Visual<'a>,
}

enum Visual<'a> {
    Picture(&'a ReadyPicture, egui::Vec2),
    Status {
        galley: std::sync::Arc<egui::Galley>,
        error: Option<&'a str>,
        load: bool,
    },
}

impl<'a> Run<'a> {
    pub(super) fn measure(
        ui: &Ui,
        index: usize,
        image: &'a Image,
        images: Images<'a>,
        width: f32,
    ) -> Self {
        let width = width.max(1.);
        let cached = images.cache.get(&image.source);
        let (size, visual) = if let Some(Picture::Ready(picture)) = cached {
            let original = picture.texture.size_vec2();
            let height_limit = if images.interactive {
                f32::INFINITY
            } else {
                700.
            };
            let scale = (width / original.x).min(height_limit / original.y).min(1.);
            let pixels = original * scale;
            (pixels, Visual::Picture(picture, pixels))
        } else {
            let error = match cached {
                Some(Picture::Failed(error)) => Some(error.as_str()),
                _ if image.source.trim().is_empty() => Some("图片地址为空"),
                _ => None,
            };
            let (text, color) = match error {
                Some(error) if !images.interactive => (
                    format!("[图片：{}] {error}", image.alt),
                    Color32::from_rgb(179, 38, 30),
                ),
                Some(_) => (
                    format!("[图片：{}]", image.alt),
                    Color32::from_rgb(179, 38, 30),
                ),
                None => (
                    format!("正在加载图片：{}", image.alt),
                    Color32::from_gray(119),
                ),
            };
            let mut job = egui::text::LayoutJob::single_section(
                text,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(12.),
                    color,
                    italics: true,
                    ..Default::default()
                },
            );
            job.wrap.max_width = (width - 20.).max(1.);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            (
                galley.size() + egui::vec2(20., 12.),
                Visual::Status {
                    galley,
                    error,
                    load: cached.is_none() && error.is_none(),
                },
            )
        };
        let size = egui::vec2(size.x.max(40.).min(width), size.y.max(28.));
        let baseline = match &visual {
            Visual::Picture(_, pixels) => (size.y + pixels.y) / 2.,
            Visual::Status { galley, .. } => {
                (size.y - galley.size().y) / 2.
                    + galley
                        .rows
                        .first()
                        .and_then(|row| row.glyphs.first())
                        .map_or(0., |glyph| glyph.pos.y)
            }
        };
        Self {
            index,
            metrics: egui::epaint::text::InlineObject { size, baseline },
            image,
            visual,
        }
    }

    pub(super) fn paint(
        self,
        ui: &mut Ui,
        bounds: egui::Rect,
        id: egui::Id,
        visits: &mut Visits<'a>,
    ) {
        if !ui.is_rect_visible(bounds) {
            return;
        }
        let tooltip = if self.image.title.trim().is_empty() {
            &self.image.alt
        } else {
            &self.image.title
        };
        match self.visual {
            Visual::Picture(picture, size) => {
                let rect = egui::Rect::from_min_size(
                    bounds.left_center() - egui::vec2(0., size.y / 2.),
                    size,
                );
                ui.painter().add(egui::Shape::image(
                    picture.texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::WHITE,
                ));
                if visits.interactive {
                    if picture.is_animated() {
                        visits.animated.insert(picture.texture.id());
                    }
                    let link = self
                        .image
                        .link
                        .as_deref()
                        .filter(|link| super::external_link(link));
                    if let Some(link) = link {
                        let hit = ui
                            .interact(rect, id, egui::Sense::click())
                            .on_hover_text(tooltip)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(link));
                        }
                    } else {
                        ui.interact(bounds, id, egui::Sense::hover())
                            .on_hover_text(tooltip);
                    }
                }
            }
            Visual::Status {
                galley,
                error,
                load,
            } => {
                ui.painter().with_clip_rect(bounds).galley(
                    bounds.left_center() + egui::vec2(10., -galley.size().y / 2.),
                    galley,
                    Color32::GRAY,
                );
                if visits.interactive {
                    ui.interact(bounds, id, egui::Sense::hover())
                        .on_hover_text(error.unwrap_or(tooltip));
                    if load && visits.request.is_none() {
                        visits.request = Some(&self.image.source);
                    }
                }
            }
        }
    }
}
