//! Original n-column bead puzzle: click tube, move its top bead, undo or reset.
use egui::{Color32, Rect, RichText, Sense, Ui, pos2, vec2};
pub struct State {
    game: tomato_core::color_transpose::Game,
    dimension: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            game: Default::default(),
            dimension: "3".into(),
        }
    }
}
fn color(i: usize) -> Color32 {
    let [r, g, b] = tomato_core::color_transpose::palette(i);
    Color32::from_rgb(r, g, b)
}
pub(crate) fn show(ui: &mut Ui, state: &mut State) {
    super::ensure_bold_font(ui.ctx());
    egui::Frame::new()
        .fill(Color32::WHITE)
        .inner_margin(15)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(10., 12.);
            ui.spacing_mut().interact_size.y = 23.;
            let (controls, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 23.), Sense::hover());
            let rect = Rect::from_center_size(controls.center(), vec2(267., 23.));
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "维度 n：",
                crate::theme::java_font(12.),
                Color32::BLACK,
            );
            let field = Rect::from_min_size(rect.min + vec2(56., 0.), vec2(60., 23.));
            ui.put(
                field,
                egui::TextEdit::singleline(&mut state.dimension)
                    .font(crate::theme::java_font(12.))
                    .margin(egui::Margin::symmetric(8, 2))
                    .char_limit(32),
            );
            let start = Rect::from_min_size(rect.min + vec2(126., 0.), vec2(40., 23.));
            let response = ui.interact(start, ui.id().with("transpose-start"), Sense::click());
            ui.painter()
                .rect_filled(start, 4., Color32::from_rgb(7, 193, 96));
            ui.painter().text(
                start.center(),
                egui::Align2::CENTER_CENTER,
                "开始",
                crate::theme::java_font(12.),
                Color32::WHITE,
            );
            let begin = activated(ui, &response);
            let undo = crate::native_dialog::button(
                ui,
                Rect::from_min_size(rect.min + vec2(176., 0.), vec2(40., 23.)),
                "撤销",
                false,
            );
            if activated(ui, &undo) {
                state.game.undo();
            }
            let reset = crate::native_dialog::button(
                ui,
                Rect::from_min_size(rect.min + vec2(226., 0.), vec2(40., 23.)),
                "重置",
                false,
            );
            if begin || activated(ui, &reset) {
                state.game.start(&state.dimension);
                state.dimension = state.game.dimension.to_string();
            }
            ui.advance_cursor_after_rect(controls);
            let n = state.game.dimension;
            let (goal, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.), Sense::hover());
            let label = "目标（每个容器同色）：";
            let galley = ui.painter().layout_no_wrap(
                label.into(),
                crate::theme::java_font(12.),
                Color32::from_gray(136),
            );
            let width = galley.size().x + 8. + (n * 30 - 6) as f32;
            let x = goal.center().x - width / 2.;
            ui.painter().galley(
                pos2(x, goal.center().y - galley.size().y / 2.),
                galley,
                Color32::from_gray(136),
            );
            let start = x + width - (n * 30 - 6) as f32;
            for i in 0..n {
                let p = pos2(start + i as f32 * 30. + 12., goal.center().y);
                ui.painter().circle_filled(p, 11., color(i));
                ui.painter()
                    .circle_stroke(p, 11., egui::Stroke::new(1.2, Color32::WHITE));
            }
            let height = (ui.available_height() - 30.).max(60.);
            let (board, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
            let top = board.center() - vec2((n * 34 - 4) as f32 / 2., (n * 32 - 2) as f32 / 2.);
            let painter = ui.painter().with_clip_rect(board.intersect(ui.clip_rect()));
            let mut clicked = None;
            for c in 0..n {
                let tube = Rect::from_min_size(
                    top + vec2(c as f32 * 34., 0.),
                    vec2(30., (n * 32 - 2) as f32),
                );
                painter.rect_filled(tube, 6., Color32::from_gray(250));
                painter.rect_stroke(
                    tube,
                    6.,
                    egui::Stroke::new(1., Color32::from_gray(238)),
                    egui::StrokeKind::Inside,
                );
                if ui
                    .interact(tube, ui.id().with(("transpose-tube", c)), Sense::click())
                    .clicked()
                {
                    clicked = Some(c);
                }
                let beads = &state.game.columns[c];
                for slot in 0..n {
                    let index = n - 1 - slot;
                    let mut center = tube.min + vec2(15., slot as f32 * 32. + 15.);
                    if let Some(&bead) = beads.get(index) {
                        if state.game.selected == Some(c) && index + 1 == beads.len() {
                            center.y -= 6.;
                        }
                        let fill = color(bead as usize);
                        painter.circle_filled(center, 14., fill);
                        painter.text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            (bead + 1).to_string(),
                            egui::FontId::new(11., egui::FontFamily::Name("md-bold".into())),
                            Color32::from_rgb(255 - fill.r(), 255 - fill.g(), 255 - fill.b()),
                        );
                    } else {
                        for segment in 0..17 {
                            let begin = segment as f32 * 5. / 13.5;
                            let points = (0..=3)
                                .map(|i| {
                                    center
                                        + vec2(
                                            (begin + i as f32 * 2. / 3. / 13.5).cos(),
                                            (begin + i as f32 * 2. / 3. / 13.5).sin(),
                                        ) * 13.5
                                })
                                .collect();
                            painter.add(egui::Shape::line(
                                points,
                                egui::Stroke::new(1., Color32::from_gray(224)),
                            ));
                        }
                    }
                }
            }
            if let Some(c) = clicked {
                state.game.click(c);
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                let text = RichText::new(&state.game.status)
                    .size(if state.game.victory_style { 14. } else { 13. })
                    .color(if state.game.victory_style {
                        Color32::from_rgb(7, 193, 96)
                    } else {
                        Color32::from_gray(102)
                    });
                ui.label(if state.game.victory_style {
                    text.strong()
                } else {
                    text
                });
            });
        });
}

impl State {
    pub(crate) fn fixture(mode: &str) -> Self {
        let mut state = Self::default();
        if mode == "selected" || mode == "moved" {
            state.game.click(0);
        }
        if mode == "moved" {
            state.game.click(2);
        }
        state
    }
}

fn activated(ui: &Ui, response: &egui::Response) -> bool {
    response.clicked()
        || response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, state: &mut State, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(896., 650.))),
                events,
                ..Default::default()
            },
            |ui| show(ui, state),
        );
        output.textures_delta.clear();
        output
    }
    fn click(ctx: &egui::Context, state: &mut State, p: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    #[test]
    fn actual_tube_pointer_moves_top_bead_and_undo_reset_controls_work() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut fonts = egui::FontDefinitions::default();
        let family = fonts.families[&egui::FontFamily::Proportional].clone();
        fonts
            .families
            .insert(egui::FontFamily::Name("java-ui".into()), family);
        let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
        fonts
            .families
            .insert(egui::FontFamily::Name("md-bold".into()), fallback);
        crate::font_assets::install(&ctx, fonts);
        let mut state = State::default();
        let output = frame(&ctx, &mut state, vec![]);
        let bead = output
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Circle(c) = &s.shape
                    && c.radius == 14.
                    && c.fill == color(2)
                {
                    Some(c.center)
                } else {
                    None
                }
            })
            .unwrap();
        click(&ctx, &mut state, bead);
        assert_eq!(state.game.selected, Some(0));
        click(&ctx, &mut state, bead + vec2(68., 0.));
        assert_eq!(state.game.columns[2], vec![2]);
        assert_eq!(state.game.moves, 1);
        for label in ["撤销", "重置"] {
            let output = frame(&ctx, &mut state, vec![]);
            let p = output
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape
                        && t.galley.job.text == label
                    {
                        Some(t.pos + t.galley.size() / 2.)
                    } else {
                        None
                    }
                })
                .unwrap();
            click(&ctx, &mut state, p);
            assert_eq!(state.game.moves, 0);
            assert!(state.game.columns[2].is_empty());
        }
    }
}
