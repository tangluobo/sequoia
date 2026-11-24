//! Message metadata: a measured key column and the remaining width for values.
use egui::{Color32, RichText, Ui, vec2};
use serde_json::Value;

fn copyable(key: &str) -> bool {
    matches!(key, "offset" | "msgId" | "queueOffset")
}

pub(super) fn render(ui: &mut Ui, detail: &str, icons: &mut crate::icons::Icons) {
    egui::Frame::new()
        .fill(Color32::from_gray(248))
        .stroke(egui::Stroke::new(1., Color32::from_gray(224)))
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(7.);
            let Ok(fields) = serde_json::from_str::<Value>(detail) else {
                return;
            };
            let Some(values) = fields.as_object() else {
                return;
            };
            let font = crate::theme::java_font(12.);
            // A horizontal Ui inside Grid is greedy: its key cell can consume
            // the entire panel, leaving the value just Grid's minimum width.
            // Measure keys first, then constrain that cell explicitly.
            let key_width = values
                .keys()
                .filter(|key| key.as_str() != "body")
                .map(|key| {
                    let text = RichText::new(format!("{key}:")).font(font.clone()).strong();
                    egui::WidgetText::from(text)
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Extend),
                            f32::INFINITY,
                            font.clone(),
                        )
                        .size()
                        .x
                        .ceil()
                        + if copyable(key) { 20. } else { 0. }
                })
                .fold(0., f32::max);
            // Leave usable value space even when a broker returns an unusually
            // long custom property name. Hovering still exposes the whole key.
            let key_width = key_width.min((ui.available_width() * 0.45).max(1.));
            ui.spacing_mut().item_spacing = vec2(8., 4.);
            ui.spacing_mut().interact_size.y = 16.;
            for (key, value) in values.iter().filter(|(key, _)| key.as_str() != "body") {
                let value = if key == "headers" {
                    value
                        .as_array()
                        .map(|headers| {
                            headers
                                .iter()
                                .map(|header| {
                                    format!(
                                        "{}={}",
                                        super::text(header, "key"),
                                        super::text(header, "value")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                } else {
                    super::text(&fields, key)
                };
                let value_width = (ui.available_width() - key_width - 8.).max(1.);
                let value_font = if key == "headers" {
                    egui::FontId::monospace(12.)
                } else {
                    font.clone()
                };
                let galley = egui::WidgetText::from(RichText::new(value).font(value_font.clone()))
                    .into_galley(ui, Some(egui::TextWrapMode::Wrap), value_width, value_font);
                let row_height = galley.size().y.max(16.);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), row_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(row_height);
                        ui.allocate_ui_with_layout(
                            vec2(key_width, row_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(key_width);
                                ui.set_min_height(row_height);
                                ui.spacing_mut().item_spacing.x = 2.;
                                ui.spacing_mut().button_padding = vec2(2., 1.);
                                ui.spacing_mut().interact_size = vec2(18., 16.);
                                let label_width =
                                    (key_width - if copyable(key) { 20. } else { 0. }).max(1.);
                                ui.allocate_ui_with_layout(
                                    vec2(label_width, 16.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(label_width);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!("{key}:"))
                                                    .font(font.clone())
                                                    .strong(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(key);
                                    },
                                );
                                if copyable(key)
                                    && let Some(icon) = icons.get(ui.ctx(), "images/copy.png")
                                    && ui
                                        .add(
                                            egui::Button::image(egui::Image::new((
                                                icon,
                                                vec2(14., 14.),
                                            )))
                                            .frame(false),
                                        )
                                        .on_hover_text("复制")
                                        .clicked()
                                {
                                    ui.ctx().copy_text(super::text(&fields, key));
                                }
                            },
                        );
                        // Wrap against the remaining panel width, preserving explicit
                        // header newlines and allowing selection/copy of long values.
                        ui.allocate_ui_with_layout(
                            vec2(value_width, row_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.add(egui::Label::new(galley).selectable(true));
                            },
                        );
                    },
                );
            }
        });
}
