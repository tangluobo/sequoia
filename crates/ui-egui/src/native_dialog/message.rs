//! OpenJFX alert factory. Geometry comes from Modena CSS and owned Java scenes.
use super::{button, font, gradient};
use egui::{Color32, Rect};
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Confirmation,
    Information,
    Warning,
    Error,
}
impl Kind {
    fn asset(self) -> &'static str {
        match self {
            Self::Confirmation => "upstream/openjfx/dialog-confirm.png",
            Self::Information => "upstream/openjfx/dialog-information.png",
            Self::Warning => "upstream/openjfx/dialog-warning.png",
            Self::Error => "upstream/openjfx/dialog-error.png",
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Role {
    Other,
    Ok,
    Yes,
    No,
    Cancel,
}
impl Role {
    fn custom(self) -> bool {
        matches!(self, Self::Other)
    }
    fn default(self) -> bool {
        matches!(self, Self::Ok | Self::Yes)
    }
    fn cancel(self) -> bool {
        matches!(self, Self::No | Self::Cancel)
    }
}
pub(crate) struct Button<'a> {
    pub label: &'a str,
    pub role: Role,
}
pub(crate) struct Message<'a> {
    pub id: egui::ViewportId,
    pub owner: Option<Rect>,
    pub title: &'a str,
    pub header: Option<&'a str>,
    pub content: &'a str,
    pub kind: Kind,
    pub buttons: &'a [Button<'a>],
    pub active: bool,
}
pub(crate) enum Answer {
    Button(usize),
    Closed,
}
struct Metrics {
    width: f32,
    height: f32,
    header: f32,
    body: f32,
}
fn rows(ctx: &egui::Context, text: &str, size: f32, width: f32) -> usize {
    let font_id = font(ctx, size);
    ctx.fonts_mut(|fonts| {
        fonts
            .layout(text.to_owned(), font_id, Color32::BLACK, width)
            .rows
            .len()
            .max(1)
    })
}
fn metrics(ctx: &egui::Context, message: &Message<'_>) -> Metrics {
    let header = message
        .header
        .filter(|text| !text.is_empty())
        .map_or(0., |text| {
            20. + 48_f32.max(rows(ctx, text, 14., 282.) as f32 * 17.)
        });
    let body = 16. + rows(ctx, message.content, 12., 340.) as f32 * 15.;
    let body = if header == 0. { 58_f32.max(body) } else { body };
    Metrics {
        width: if header == 0. { 418. } else { 360. },
        height: header + body + 43.,
        header,
        body,
    }
}
pub(crate) fn show(
    ctx: &egui::Context,
    message: Message<'_>,
    focus: &mut bool,
    icons: &mut crate::icons::Icons,
    mut observe: impl FnMut(&egui::Ui, &[Rect]),
) -> Option<Answer> {
    let metrics = metrics(ctx, &message);
    let builder = egui::ViewportBuilder::default()
        .with_title(message.title)
        .with_inner_size([metrics.width, metrics.height])
        .with_resizable(false)
        .with_taskbar(false)
        .with_active(message.active);
    let builder = super::placement::prepare(
        ctx,
        message.id,
        builder,
        *focus,
        message.owner,
        egui::vec2(metrics.width, metrics.height),
    );
    let mut answer = None;
    ctx.show_viewport_immediate(message.id, builder, |ui, _| {
        super::placement::correct(ui, message.id);
        let origin = ui.max_rect().min;
        let at = |x, y, w, h| Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
        ui.painter()
            .rect_filled(ui.max_rect(), 0, Color32::from_gray(244));
        if metrics.header > 0. {
            gradient(
                ui,
                at(0., 0., metrics.width, metrics.header),
                Color32::from_gray(244),
                Color32::from_gray(249),
            );
            ui.painter().line_segment(
                [
                    origin + egui::vec2(0., metrics.header - 0.5),
                    origin + egui::vec2(metrics.width, metrics.header - 0.5),
                ],
                egui::Stroke::new(1., Color32::from_gray(200)),
            );
            icons.paint(
                ui,
                message.kind.asset(),
                at(metrics.width - 58., 10., 48., 48.),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(at(10., 10., 282., metrics.header - 20.))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(message.header.unwrap_or_default())
                                .font(font(ctx, 14.))
                                .color(Color32::BLACK),
                        )
                        .wrap(),
                    );
                },
            );
        } else {
            icons.paint(ui, message.kind.asset(), at(10., 10., 48., 48.));
        }
        let x = if metrics.header > 0. { 10. } else { 68. };
        ui.scope_builder(
            egui::UiBuilder::new()
                // System 12's glyph baseline is one pixel above ab_glyph's.
                .max_rect(at(x, metrics.header + 15., 340., metrics.body - 15.))
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(message.content)
                            .font(font(ctx, 12.))
                            .line_height(Some(15.))
                            .color(Color32::BLACK),
                    )
                    .wrap(),
                );
            },
        );
        let count = message.buttons.len();
        let initial_focus = message
            .buttons
            .iter()
            .position(|button| button.role.default())
            .or((count > 0).then_some(0));
        let mut rects = Vec::with_capacity(count);
        for (index, spec) in message.buttons.iter().enumerate() {
            let x = if count == 2 && index == 0 && spec.role.custom() {
                (metrics.width - 160.) / 2.
            } else {
                metrics.width - 10. - count as f32 * 75. - count.saturating_sub(1) as f32 * 10.
                    + index as f32 * 85.
            };
            let rect = at(x, metrics.height - 33., 75., 23.);
            rects.push(rect);
            let response = button(ui, rect, spec.label, spec.role.default());
            if *focus && initial_focus == Some(index) {
                response.request_focus();
            }
            if response.clicked()
                || response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
            {
                answer = Some(Answer::Button(index));
            }
        }
        *focus = false;
        if ui.input(|input| input.key_pressed(egui::Key::Enter))
            && answer.is_none()
            && let Some(index) = message
                .buttons
                .iter()
                .position(|button| button.role.default())
        {
            answer = Some(Answer::Button(index));
        }
        if ui.input(|input| {
            input.key_pressed(egui::Key::Escape) || input.viewport().close_requested()
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            answer = Some(
                message
                    .buttons
                    .iter()
                    .position(|button| button.role.cancel())
                    .map_or(Answer::Closed, Answer::Button),
            );
        }
        observe(ui, &rects);
    });
    answer
}
