use anyhow::{Context as _, Result, ensure};
use egui::{
    ClippedPrimitive, Context, FontDefinitions, Id, RawInput, Rect, Shape, TextureId,
    epaint::ImageDelta, pos2, vec2,
};
use std::{collections::HashMap, ops::Range};
use tomato_services::markdown::{Document, Picture};

pub(super) const SHEET: [f32; 2] = [595.28, 841.89];
pub(super) const SCALE: f32 = 3.;
const LEFT: f32 = 36. + 16.;
const TOP: f32 = 36. + 12.;
const WIDTH: f32 = SHEET[0] - 2. * LEFT;
const HEIGHT: f32 = SHEET[1] - 2. * TOP;
const MAX_PAGES: usize = 250;

pub(super) struct Layout {
    context: Context,
    _images: HashMap<String, crate::tabs::markdown::Picture>,
    shapes: Vec<(egui::epaint::ClippedShape, Rect)>,
    pub textures: Vec<(TextureId, ImageDelta)>,
    pages: Vec<Range<f32>>,
}

impl Layout {
    pub fn new(
        document: Document,
        fonts: FontDefinitions,
        images: HashMap<String, Result<Picture>>,
        max_texture_side: usize,
    ) -> Result<Self> {
        let context = Context::default();
        context.set_fonts(fonts);
        context.set_pixels_per_point(SCALE);
        // A print worker uses the already-resolved faces and grayscale glyph coverage.
        context.data_mut(|data| data.insert_temp(Id::new("md-bold-cjk-requested"), true));
        let images = images
            .into_iter()
            .map(|(name, image)| {
                let entry = match image {
                    Ok(image) => crate::tabs::markdown::Picture::Ready(
                        crate::tabs::markdown::ReadyPicture::new(&context, image),
                    ),
                    Err(error) => crate::tabs::markdown::Picture::Failed(error.to_string()),
                };
                (name, entry)
            })
            .collect();
        let maximum = HEIGHT * MAX_PAGES as f32;
        let rect = Rect::from_min_size(pos2(0., 0.), vec2(WIDTH, maximum));
        let mut ends = None;
        let mut output = context.run_ui(
            RawInput {
                screen_rect: Some(rect),
                max_texture_side: Some(max_texture_side),
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(rect);
                ui.spacing_mut().item_spacing.y = 0.;
                ends = Some(crate::panels::print_document(
                    ui, &document, &images, maximum,
                ));
            },
        );
        let textures = output
            .textures_delta
            .set
            .drain()
            .flat_map(|(id, deltas)| deltas.into_iter().map(move |delta| (id, delta)))
            .collect();
        output.textures_delta.clear();
        let ends = ends.context("未生成 Markdown 打印布局")??;
        ensure!(!ends.is_empty(), "没有可打印的内容");
        let mut protected = Vec::new();
        let shapes = std::mem::take(&mut output.shapes)
            .into_iter()
            .filter_map(|shape| {
                let bounds = shape
                    .shape
                    .visual_bounding_rect()
                    .intersect(shape.clip_rect);
                if !bounds.is_positive() {
                    return None;
                }
                protected_intervals(&shape.shape, shape.clip_rect, &mut protected);
                Some((shape, bounds))
            })
            .collect();
        let pages = paginate(&ends, protected, HEIGHT, MAX_PAGES)?;
        Ok(Self {
            context,
            _images: images,
            shapes,
            textures,
            pages,
        })
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn primitives(&self, index: usize) -> Vec<ClippedPrimitive> {
        let page = &self.pages[index];
        let slice = Rect::from_min_max(pos2(0., page.start), pos2(WIDTH, page.end));
        let delta = vec2(LEFT, TOP - page.start);
        let clipped = self
            .shapes
            .iter()
            .filter(|(_, bounds)| bounds.intersects(slice))
            .map(|(shape, _)| {
                let mut shape = shape.clone();
                shape.clip_rect = shape.clip_rect.intersect(slice).translate(delta);
                shape.shape.translate(delta);
                shape
            })
            .collect();
        self.context.tessellate(clipped, SCALE)
    }
}

fn protected_intervals(shape: &Shape, clip: Rect, protected: &mut Vec<Range<f32>>) {
    let mut protect = |top: f32, bottom: f32| {
        let start = top.max(clip.top());
        let end = bottom.min(clip.bottom());
        if start < end {
            protected.push(start..end);
        }
    };
    match shape {
        Shape::Text(text) => {
            for row in &text.galley.rows {
                let top = text.pos.y + row.pos.y;
                protect(top, top + row.size.y);
            }
        }
        Shape::Mesh(_) => {
            let rect = shape.visual_bounding_rect();
            protect(rect.top(), rect.bottom());
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                protected_intervals(shape, clip, protected);
            }
        }
        _ => {}
    }
}

fn paginate(
    ends: &[f32],
    mut protected: Vec<Range<f32>>,
    page_height: f32,
    limit: usize,
) -> Result<Vec<Range<f32>>> {
    protected.sort_unstable_by(|a, b| a.start.total_cmp(&b.start));
    let mut merged: Vec<Range<f32>> = Vec::new();
    for interval in protected {
        if let Some(last) = merged.last_mut()
            && interval.start < last.end
        {
            last.end = last.end.max(interval.end);
        } else {
            merged.push(interval);
        }
    }
    let height = ends.last().copied().unwrap_or_default();
    let mut pages = Vec::new();
    let mut start = 0.;
    while start < height {
        ensure!(pages.len() < limit, "Markdown 超过打印页数上限");
        let cap = (start + page_height).min(height);
        let block_end = ends[..ends.partition_point(|value| *value <= cap)]
            .last()
            .copied();
        let mut end = block_end
            .filter(|end| *end > start && cap - *end <= page_height * 0.25)
            .unwrap_or(cap);
        let at = merged.partition_point(|interval| interval.start < end);
        if let Some(interval) = merged[..at].last()
            && interval.start > start
            && end < interval.end
        {
            end = interval.start;
        }
        ensure!(end > start, "Markdown 分页无法继续");
        pages.push(start..end);
        start = end;
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_keep_rows_whole_and_continue_after_large_blocks() -> Result<()> {
        assert_eq!(
            paginate(&[280.], vec![93.0..108.0, 193.0..207.0], 100., 4)?,
            vec![0.0..93.0, 93.0..193.0, 193.0..280.0]
        );
        assert!(paginate(&[501.], vec![], 100., 5).is_err());
        Ok(())
    }
}
