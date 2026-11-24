//! Fixed JavaFX menu metrics, independent of the table cell's layout and style.
use egui::{Align2, Color32, FontId, Response, Sense, Ui, Widget, vec2};
pub(crate) const ROW_HEIGHT: f32 = 23.;
pub(crate) struct Item<'a> {
    label: &'a str,
    icon: Option<egui::TextureId>,
    strong: bool,
    table: bool,
    icon_column: bool,
    max_width: Option<f32>,
}
impl<'a> Item<'a> {
    pub(crate) fn new(label: &'a str) -> Self {
        Self {
            label,
            icon: None,
            strong: false,
            table: false,
            icon_column: false,
            max_width: None,
        }
    }
    pub(crate) fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub(crate) fn icon_column(mut self) -> Self {
        self.icon_column = true;
        self
    }
    pub(crate) fn table(mut self) -> Self {
        self.table = true;
        self
    }
    pub(crate) fn strong(mut self) -> Self {
        self.strong = true;
        self
    }
    pub(crate) fn icon(mut self, icon: Option<egui::TextureId>) -> Self {
        self.icon = icon;
        self
    }
}
impl Widget for Item<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.spacing_mut().item_spacing.y = 0.;
        let size = if self.table { 10. } else { 12. };
        let padding = if self.table { 4. } else { 8. };
        let regular_font = if self.table {
            crate::native_dialog::font(ui.ctx(), size)
        } else {
            FontId::proportional(size)
        };
        let font = if self.strong
            && ui.ctx().fonts(|fonts| {
                fonts
                    .definitions()
                    .families
                    .contains_key(&egui::FontFamily::Name("table-bold".into()))
            }) {
            FontId::new(size, egui::FontFamily::Name("table-bold".into()))
        } else {
            regular_font
        };
        let left = if self.icon.is_some() || self.icon_column {
            23. + padding
        } else {
            padding
        };
        let width = self.max_width.map_or_else(
            || {
                let natural = ui
                    .painter()
                    .layout_no_wrap(self.label.to_owned(), font.clone(), Color32::BLACK)
                    .size()
                    .x
                    + left
                    + if self.icon.is_some() || self.icon_column {
                        4.
                    } else {
                        padding
                    };
                // A new popup starts with a suggested area. Let its contents
                // establish the width before stretching rows to fill it.
                let available = if ui.is_sizing_pass() {
                    0.
                } else {
                    ui.available_width()
                };
                available
                    .max(natural)
                    .max(if self.table { 0. } else { 150. })
            },
            |maximum| ui.available_width().min(maximum),
        );
        let height = if self.table { 20. } else { ROW_HEIGHT };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        let hover = response.hovered() && ui.is_enabled();
        if hover {
            ui.painter()
                .rect_filled(rect, 0., Color32::from_rgb(0, 150, 201));
        }
        let color = if !ui.is_enabled() {
            Color32::from_gray(if self.table { 51 } else { 153 })
        } else if hover {
            Color32::WHITE
        } else if self.table {
            Color32::from_gray(51)
        } else {
            Color32::BLACK
        };
        if let Some(icon) = self.icon {
            ui.painter().image(
                icon,
                egui::Rect::from_center_size(
                    rect.left_center() + vec2(13., 0.),
                    vec2(if self.table { 18. } else { 16. }, 16.),
                ),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        let response = if self.max_width.is_some() {
            let galley = single_line(
                ui,
                self.label,
                font,
                color,
                (width - left - padding).max(0.),
            );
            let elided = galley.elided;
            ui.painter().galley(
                rect.left_center() + vec2(left, -galley.size().y / 2.),
                galley,
                color,
            );
            if !elided {
                response
            } else if ui.is_enabled() {
                response.on_hover_text(self.label)
            } else {
                response.on_disabled_hover_text(self.label)
            }
        } else {
            ui.painter().text(
                rect.left_center() + vec2(left, 0.),
                Align2::LEFT_CENTER,
                self.label,
                font,
                color,
            );
            response
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label)
        });
        response
    }
}

pub(crate) fn single_line(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

pub(crate) fn separator(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.;
    let width = if ui.is_sizing_pass() {
        0.
    } else {
        ui.available_width()
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, 3.), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1., Color32::from_gray(204)),
    );
}

#[cfg(test)]
pub(crate) fn assert_actual_rows(draw: impl FnMut(&mut Ui), point: egui::Pos2, labels: &[&str]) {
    assert_rows_at_height(draw, point, labels, ROW_HEIGHT);
}
#[cfg(test)]
pub(crate) fn assert_rows_at_height(
    mut draw: impl FnMut(&mut Ui),
    point: egui::Pos2,
    labels: &[&str],
    height: f32,
) {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut run = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(1000., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.spacing_mut().interact_size.y = 100.;
                draw(ui);
            },
        );
        output.textures_delta.clear();
        output
    };
    run(vec![]);
    for pressed in [true, false] {
        run(vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    let output = run(vec![]);
    let positions: Vec<_> = labels
        .iter()
        .map(|label| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == *label => Some(text.pos.y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Actual menu item missing: {label}"))
        })
        .collect();
    for pair in positions.windows(2) {
        assert!(
            (pair[1] - pair[0] - height).abs() < 0.1,
            "Menu row positions: {positions:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_rows_do_not_inherit_cell_height_or_wrap() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut heights = vec![];
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.spacing_mut().interact_size.y = 100.;
            ui.spacing_mut().button_padding = vec2(40., 40.);
            ui.set_width(180.);
            for label in ["打开表", "设计表", "设置为空字符串", "复制字段"] {
                heights.push(ui.add(Item::new(label)).rect.height());
            }
        });
        output.textures_delta.clear();
        assert_eq!(heights, vec![ROW_HEIGHT; 4]);
    }
}
