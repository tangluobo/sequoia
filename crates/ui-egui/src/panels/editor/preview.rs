use super::*;
use crate::tabs::markdown::{Picture, State, TableGeometry};
use std::collections::{HashMap, HashSet};
use tomato_services::markdown::{Block, Kind, Span};
mod image;
mod table;
#[cfg(test)]
mod tests;
fn code(ui: &mut Ui, block: &Block, language: &str, interactive: bool) {
    let text = block
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    let lines: Vec<_> = text.split('\n').collect();
    // A common Latin/CJK line pitch also keeps code numbers aligned when only
    // some lines need the CJK fallback face. The tiny metrics galley is cached.
    let metrics = ui.fonts_mut(|fonts| fonts.layout_job(super::highlight::code("Ag中文", "", 12.)));
    let row_height = metrics.size().y;
    let baseline = metrics.rows[0].glyphs[0].pos.y;
    let (rect, _) = ui.allocate_exact_size(
        // Java's line-number TextFlow retains its final newline as an empty row.
        egui::vec2(
            ui.available_width(),
            (lines.len() + 1) as f32 * row_height + 16.,
        ),
        egui::Sense::hover(),
    );
    let rect = rect.shrink2(egui::vec2(5., 0.));
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 4., Color32::from_rgb(246, 248, 250));
    painter.rect_stroke(
        rect,
        4.,
        egui::Stroke::new(1., Color32::from_gray(224)),
        egui::StrokeKind::Inside,
    );
    let first = ((ui.clip_rect().top() - rect.top() - 8.) / row_height)
        .floor()
        .max(0.) as usize;
    let last = (((ui.clip_rect().bottom() - rect.top()) / row_height)
        .ceil()
        .max(0.) as usize)
        .min(lines.len());
    let number_width = lines.len().to_string().len() as f32 * 7.;
    for (index, line) in lines.iter().enumerate().take(last).skip(first) {
        let y = rect.top() + 8. + index as f32 * row_height;
        super::highlight::line_number(
            ui,
            index + 1,
            egui::pos2(rect.left() + 4. + number_width, y),
            Some(baseline),
            12.,
            Color32::from_gray(170),
        );
        let job = super::highlight::code(line, language, 12.);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let own_baseline = galley
            .rows
            .first()
            .and_then(|row| row.glyphs.first())
            .map_or(baseline, |glyph| glyph.pos.y);
        painter.galley(
            egui::pos2(
                rect.left() + 4. + number_width + 18.,
                y + baseline - own_baseline,
            ),
            galley,
            Color32::from_gray(8),
        );
    }
    if !interactive {
        return;
    }
    let button = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 26., rect.top() + 4.),
        egui::vec2(20., 20.),
    );
    let feedback_id = ui.id().with("code-copied");
    let now = ui.input(|input| input.time);
    let until = ui
        .data(|data| data.get_temp::<f64>(feedback_id))
        .unwrap_or(0.);
    let copied = now < until;
    if copied {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(until - now));
    }
    if ui.is_rect_visible(button)
        && ui
            .put(
                button,
                egui::Button::new(
                    RichText::new(if copied { "\u{f00c}" } else { "\u{f0c5}" })
                        .color(if copied {
                            Color32::from_rgb(40, 167, 69)
                        } else {
                            Color32::from_gray(136)
                        })
                        .font(egui::FontId::new(
                            12.,
                            egui::FontFamily::Name("fontawesome".into()),
                        )),
                )
                .frame(false),
            )
            .on_hover_text("复制代码")
            .clicked()
    {
        ui.ctx().copy_text(text);
        ui.data_mut(|data| data.insert_temp(feedback_id, now + 1.2));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1200));
    }
}
struct Inline<'a> {
    galley: std::sync::Arc<egui::Galley>,
    links: Vec<(std::ops::Range<usize>, &'a str)>,
    images: Vec<image::Run<'a>>,
}

#[derive(Clone, Copy)]
struct Images<'a> {
    cache: &'a HashMap<String, Picture>,
    interactive: bool,
}

#[derive(Default)]
struct Visits<'a> {
    interactive: bool,
    request: Option<&'a str>,
    animated: HashSet<egui::TextureId>,
}

fn text_layout<'a>(
    ui: &Ui,
    spans: &'a [Span],
    size: f32,
    bold: bool,
    width: f32,
    assets: Images<'a>,
) -> Inline<'a> {
    if spans
        .iter()
        .any(|span| (bold || span.style.bold) && !span.text.is_ascii())
    {
        super::highlight::ensure_bold_cjk(ui.ctx());
    }
    // Java TextFlow aligns ordinary, bold, code and fallback glyphs on one
    // baseline, rather than aligning the bottoms of their different line boxes.
    let mut job = egui::text::LayoutJob {
        align_on_baseline: true,
        ..Default::default()
    };
    let mut links = Vec::new();
    let mut images = Vec::new();
    let mut chars = 0;
    for span in spans {
        if let Some(image) = &span.image {
            let run =
                image::Run::measure(ui, chars, image, assets, width.min(ui.available_width()));
            job.append(
                "\u{fffc}",
                0.,
                egui::TextFormat {
                    inline_object: Some(run.metrics),
                    color: Color32::TRANSPARENT,
                    ..Default::default()
                },
            );
            images.push(run);
            chars += 1;
            continue;
        }
        let color = if span.style.link.is_some() {
            Color32::from_rgb(26, 115, 232)
        } else if span.style.code {
            Color32::from_rgb(199, 37, 78)
        } else {
            Color32::from_gray(51)
        };
        let format = egui::TextFormat {
            font_id: egui::FontId::new(
                size,
                if span.style.code {
                    egui::FontFamily::Monospace
                } else if bold || span.style.bold {
                    egui::FontFamily::Name("md-bold".into())
                } else {
                    egui::FontFamily::Proportional
                },
            ),
            color,
            line_height: (size == 13.).then_some(16.5),
            italics: span.style.italic,
            strikethrough: if span.style.strike {
                egui::Stroke::new(1., color)
            } else {
                egui::Stroke::NONE
            },
            underline: if span.style.link.is_some() {
                egui::Stroke::new(1., color)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        };
        let end = chars + span.text.chars().count();
        if let Some(url) = &span.style.link {
            links.push((chars..end, url.as_str()));
        }
        chars = end;
        job.append(&span.text, 0., format);
    }
    job.wrap.max_width = width;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    Inline {
        galley,
        links,
        images,
    }
}
fn inline<'a>(
    ui: &mut Ui,
    spans: &'a [Span],
    size: f32,
    bold: bool,
    images: Images<'a>,
    visits: &mut Visits<'a>,
) {
    let layout = text_layout(ui, spans, size, bold, ui.available_width(), images);
    paint_inline(ui, layout, visits);
}
fn external_link(url: &str) -> bool {
    ["https://", "http://", "mailto:"].iter().any(|prefix| {
        url.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    })
}

fn paint_inline<'a>(ui: &mut Ui, layout: Inline<'a>, visits: &mut Visits<'a>) {
    let Inline {
        galley,
        links,
        images,
    } = layout;
    let response = ui.add(
        egui::Label::new(galley.clone())
            .selectable(true)
            .sense(egui::Sense::click()),
    );
    if visits.interactive {
        response.context_menu(|ui| {
            if ui.add(crate::compact_menu::Item::new("复制整段")).clicked() {
                ui.ctx()
                    .copy_text(galley.job.text_without_objects(0..galley.job.text.len()));
                ui.close();
            }
        });
    }
    if visits.interactive
        && let Some(pointer) = response.hover_pos()
    {
        let index: usize = galley
            .cursor_from_pos(pointer - response.rect.min)
            .index
            .into();
        if let Some((_, url)) = links.iter().find(|(range, _)| range.contains(&index)) {
            response
                .clone()
                .on_hover_text(*url)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() && external_link(url) {
                ui.ctx().open_url(egui::OpenUrl::new_tab(*url));
            }
        }
    }
    let mut images = images.into_iter().peekable();
    let mut start = 0;
    for row in &galley.rows {
        let end = start + row.glyphs.len();
        while let Some(image) = images.next_if(|image| image.index < end) {
            let glyph = &row.glyphs[image.index - start];
            let origin = response.rect.min + row.pos.to_vec2() + glyph.pos.to_vec2()
                - egui::vec2(0., image.metrics.baseline);
            let bounds = egui::Rect::from_min_size(origin, image.metrics.size);
            let id = response.id.with(("picture", image.index));
            image.paint(ui, bounds, id, visits);
        }
        if images.peek().is_none() {
            break;
        }
        start = end + usize::from(row.ends_with_newline);
    }
}
fn block<'a>(
    ui: &mut Ui,
    block: &'a Block,
    images: Images<'a>,
    visits: &mut Visits<'a>,
    geometry: Option<&mut TableGeometry>,
) {
    match &block.kind {
        Kind::Paragraph => inline(ui, &block.spans, 13., false, images, visits),
        Kind::Heading(level) => {
            ui.add_space(6.);
            inline(
                ui,
                &block.spans,
                [24., 20., 17., 15., 13., 12.][usize::from(level.saturating_sub(1)).min(5)],
                true,
                images,
                visits,
            );
            ui.add_space(4.);
        }
        Kind::Rule => {
            ui.add_space(8.);
            ui.separator();
            ui.add_space(8.);
        }
        Kind::Code(language) => code(ui, block, language, visits.interactive),
        Kind::HtmlBlock => {
            let text = block
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            let mut job = egui::text::LayoutJob::single_section(
                text,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(11.),
                    color: Color32::from_gray(136),
                    ..Default::default()
                },
            );
            job.align_on_baseline = true;
            ui.add(egui::Label::new(job).wrap());
        }
        Kind::Table(rows) => table::render(ui, rows, images, visits, geometry),
    }
}
fn framed_block<'a>(
    ui: &mut Ui,
    value: &'a Block,
    images: Images<'a>,
    visits: &mut Visits<'a>,
    geometry: Option<&mut TableGeometry>,
) {
    ui.spacing_mut().item_spacing.y = 0.;
    let frame = egui::Frame::new()
        .fill(if value.quote {
            Color32::from_rgb(248, 249, 250)
        } else {
            Color32::WHITE
        })
        .inner_margin(if value.quote {
            egui::Margin {
                left: 12,
                right: 0,
                top: 4,
                bottom: 4,
            }
        } else {
            egui::Margin::ZERO
        });
    let response = frame.show(ui, |ui| {
        if value.quote && !visits.interactive {
            ui.set_min_width(ui.available_width());
        }
        block(ui, value, images, visits, geometry);
    });
    if value.quote {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                response.response.rect.min,
                egui::vec2(3., response.response.rect.height()),
            ),
            0,
            Color32::from_rgb(26, 115, 232),
        );
    }
}

pub(super) fn render(ui: &mut Ui, state: &mut State) -> Option<String> {
    let drag = ui.make_persistent_id("markdown-preview-drag-copy");
    if ui.input(|i| i.pointer.primary_pressed()) {
        let origin = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| ui.clip_rect().intersect(ui.max_rect()).contains(*p));
        ui.data_mut(|d| {
            d.insert_temp(drag, origin);
        });
    }
    if ui.input(|i| i.pointer.primary_released()) {
        let origin = ui
            .data_mut(|d| d.remove_temp::<Option<egui::Pos2>>(drag))
            .flatten();
        let dragged = ui.input(|i| {
            origin
                .zip(i.pointer.interact_pos())
                .is_some_and(|(a, b)| a.distance_sq(b) > 4.)
        });
        if dragged
            && ui
                .ctx()
                .with_plugin::<egui::text_selection::LabelSelectionState, _>(|s| s.has_selection())
                .unwrap_or(false)
        {
            // Copy the actual highlighted range; a context-menu click is not a drag.
            ui.input_mut(|i| i.events.push(egui::Event::Copy));
        }
    }
    let width = ui.available_width();
    if state.heights.len() != state.document.blocks.len() || (state.width - width).abs() > 1. {
        state.width = width;
        state.table_layouts.clear();
        state.heights = state
            .document
            .blocks
            .iter()
            .map(|block| {
                let bytes: usize = block.spans.iter().map(|span| span.text.len()).sum();
                match block.kind {
                    Kind::Code(_) => {
                        block
                            .spans
                            .iter()
                            .map(|span| span.text.lines().count() as f32 * 20.)
                            .sum::<f32>()
                            + 45.
                    }
                    Kind::Table(ref rows) => rows.len() as f32 * 36.,
                    _ => ((bytes as f32 * 7. / width.max(50.)).ceil().max(1.)) * 20. + 16.,
                }
            })
            .collect();
    }
    let waiting = state
        .images
        .values()
        .any(|image| matches!(image, Picture::Pending));
    let mut visits = Visits {
        interactive: true,
        ..Default::default()
    };
    let images = Images {
        cache: &state.images,
        interactive: true,
    };
    ScrollArea::vertical()
        .id_salt("md-preview-scroll")
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            let origin = ui.cursor().min;
            let mut top = 0.;
            for (index, block_value) in state.document.blocks.iter().enumerate() {
                let height = state.heights[index];
                if top + height >= viewport.top() && top <= viewport.bottom() {
                    let rect = egui::Rect::from_min_size(
                        origin + egui::vec2(block_value.indent as f32 * 16., top),
                        egui::vec2((width - block_value.indent as f32 * 16.).max(30.), height),
                    );
                    let response = ui.push_id(index, |ui| {
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            let geometry = matches!(block_value.kind, Kind::Table(_))
                                .then(|| state.table_layouts.entry(index).or_default());
                            framed_block(ui, block_value, images, &mut visits, geometry);
                        })
                    });
                    let measured =
                        response.inner.response.rect.height() + f32::from(block_value.gap);
                    if (measured - height).abs() > 1. {
                        state.heights[index] = measured;
                        ui.ctx().request_repaint();
                    }
                }
                top += state.heights[index];
            }
            ui.set_min_height(top);
        });
    let request = visits.request.filter(|_| !waiting).map(str::to_owned);
    state.animate_visible(ui.ctx(), &visits.animated);
    if let Some(source) = &request {
        if state.images.len() >= 128 {
            state.clear_images();
        }
        state.images.insert(source.clone(), Picture::Pending);
    }
    request
}

#[cfg(windows)]
pub(crate) fn print_document(
    ui: &mut Ui,
    document: &tomato_services::markdown::Document,
    images: &HashMap<String, Picture>,
    limit: f32,
) -> anyhow::Result<Vec<f32>> {
    let mut ends = Vec::with_capacity(document.blocks.len());
    let mut visits = Visits::default();
    let images = Images {
        cache: images,
        interactive: false,
    };
    let origin = ui.cursor().min.y;
    for (index, value) in document.blocks.iter().enumerate() {
        let indent = (value.indent as f32 * 16.)
            .min(ui.available_width() - 30.)
            .max(0.);
        let rect = ui.available_rect_before_wrap();
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(index)
                .max_rect(egui::Rect::from_min_max(
                    rect.min + egui::vec2(indent, 0.),
                    rect.max,
                )),
            |ui| framed_block(ui, value, images, &mut visits, None),
        );
        ui.add_space(f32::from(value.gap));
        let height = ui.cursor().min.y - origin;
        anyhow::ensure!(height <= limit, "Markdown 超过打印页数上限");
        ends.push(height);
    }
    Ok(ends)
}
