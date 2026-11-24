//! JavaFX inner TabPane headers; shared by designers, query results and MQ pages.
use egui::{Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::hash::Hash;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) enum Style {
    Compact,
    MessageQueue,
}
impl Style {
    pub fn height(self) -> f32 {
        match self {
            Self::Compact => 25.,
            Self::MessageQueue => 33.,
        }
    }
    fn padding(self) -> f32 {
        match self {
            Self::Compact => 12.,
            Self::MessageQueue => 10.,
        }
    }
}

pub(crate) fn show<T: Copy + PartialEq, S: AsRef<str>>(
    ui: &mut Ui,
    salt: impl Hash + std::fmt::Debug,
    selected: &mut T,
    style: Style,
    items: impl IntoIterator<Item = (T, S)>,
) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), style.height()), Sense::hover());
    show_at(ui, salt, rect, selected, style, items)
}

pub(crate) fn show_at<T: Copy + PartialEq, S: AsRef<str>>(
    ui: &mut Ui,
    salt: impl Hash + std::fmt::Debug,
    rect: Rect,
    selected: &mut T,
    style: Style,
    items: impl IntoIterator<Item = (T, S)>,
) -> bool {
    let id = ui.make_persistent_id(salt);
    let font = if ui.ctx().fonts(|fonts| {
        fonts
            .definitions()
            .families
            .contains_key(&egui::FontFamily::Name("java-ui".into()))
    }) {
        crate::theme::java_font(12.)
    } else {
        egui::FontId::proportional(12.)
    };
    let items: Vec<_> = items
        .into_iter()
        .map(|(value, label)| {
            let galley = ui.painter().layout_no_wrap(
                label.as_ref().into(),
                font.clone(),
                Color32::PLACEHOLDER,
            );
            let width = galley.size().x.ceil() + style.padding() * 2. + 1.;
            (value, label, galley, width)
        })
        .collect();
    let overflow = items.iter().map(|item| item.3).sum::<f32>() > rect.width();
    let list = Rect::from_min_max(
        rect.min,
        rect.max - vec2(if overflow { 26. } else { 0. }, 0.),
    );
    ui.painter()
        .rect_filled(rect, 0, crate::theme::NAV_BACKGROUND);
    let active_key = id.with("last-active-index");
    let active = items.iter().position(|item| item.0 == *selected);
    let reveal = ui.ctx().data(|data| data.get_temp::<usize>(active_key)) != active;
    let mut changed = false;
    ui.scope_builder(egui::UiBuilder::new().id_salt(id).max_rect(list), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(list));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        ui.spacing_mut().interact_size.y = style.height();
        egui::ScrollArea::horizontal()
            .id_salt(id)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, (value, label, galley, width)) in items.iter().enumerate() {
                        let (tab, _) =
                            ui.allocate_exact_size(vec2(*width, style.height()), Sense::hover());
                        let response = ui.interact(tab, id.with(("tab", index)), Sense::click());
                        if ui
                            .ctx()
                            .data(|d| d.get_temp::<bool>(id.with("closable")))
                            .unwrap_or(false)
                        {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(id.with(("tab-rect", index)), tab));
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                ui.is_enabled(),
                                *selected == *value,
                                label.as_ref(),
                            )
                        });
                        let current = *selected == *value;
                        if reveal && current {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        ui.painter().rect_filled(
                            tab,
                            0,
                            if current {
                                Color32::from_rgb(252, 255, 255)
                            } else if response.hovered() {
                                Color32::from_gray(216)
                            } else {
                                Color32::from_gray(224)
                            },
                        );
                        // Own only the right and bottom edges: adjacent tabs never double a seam.
                        ui.painter().rect_filled(
                            Rect::from_min_max(pos2(tab.right() - 1., tab.top()), tab.max),
                            0,
                            Color32::from_gray(192),
                        );
                        ui.painter().rect_filled(
                            Rect::from_min_max(pos2(tab.left(), tab.bottom() - 1.), tab.max),
                            0,
                            if current {
                                Color32::BLACK
                            } else {
                                Color32::from_gray(192)
                            },
                        );
                        let color = if current {
                            Color32::BLACK
                        } else {
                            Color32::from_gray(85)
                        };
                        ui.painter().galley(
                            pos2(
                                tab.left() + style.padding(),
                                tab.top() + ((tab.height() - galley.size().y) * 0.5).round(),
                            ),
                            galley.clone(),
                            color.gamma_multiply(if ui.is_enabled() { 1. } else { 0.4 }),
                        );
                        if response.clicked() {
                            response.request_focus();
                            *selected = *value;
                            changed = true;
                        }
                        if Some(index) == active {
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(active_key, index));
                        }
                    }
                });
            });
    });
    if overflow {
        let button = Rect::from_min_max(pos2(list.right(), rect.top()), rect.max);
        let response = ui.interact(button, id.with("overflow"), Sense::click());
        let center = button.center();
        for y in [-2., 2.] {
            ui.painter().add(egui::Shape::line(
                vec![
                    center + vec2(-3., y - 1.),
                    center + vec2(0., y + 1.),
                    center + vec2(3., y - 1.),
                ],
                Stroke::new(1., Color32::from_gray(85)),
            ));
        }
        egui::Popup::menu(&response)
            .frame(
                egui::Frame::popup(ui.style())
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1., Color32::from_gray(192)))
                    .corner_radius(0)
                    .inner_margin(egui::Margin::symmetric(0, 3)),
            )
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                let width = items
                    .iter()
                    .map(|item| item.2.size().x.ceil())
                    .fold(0., f32::max)
                    + 32.;
                for (value, label, galley, _) in &items {
                    let (row, response) = ui.allocate_exact_size(
                        vec2(width, crate::compact_menu::ROW_HEIGHT),
                        Sense::click(),
                    );
                    let color = if response.hovered() {
                        ui.painter()
                            .rect_filled(row, 0, Color32::from_rgb(0, 150, 201));
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    };
                    if *selected == *value {
                        ui.painter().add(egui::Shape::line(
                            vec![
                                row.min + vec2(6., 11.),
                                row.min + vec2(9., 15.),
                                row.min + vec2(15., 7.),
                            ],
                            Stroke::new(1.7, color),
                        ));
                    }
                    ui.painter().galley(
                        pos2(
                            row.left() + 20.,
                            row.top() + ((row.height() - galley.size().y) * 0.5).round(),
                        ),
                        galley.clone(),
                        color,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::RadioButton,
                            ui.is_enabled(),
                            *selected == *value,
                            label.as_ref(),
                        )
                    });
                    if response.clicked() {
                        *selected = *value;
                        changed = true;
                        ui.close();
                    }
                }
            });
    }
    if ui.is_enabled() && !items.is_empty() && !egui::Popup::is_any_open(ui.ctx()) {
        let focused = ui.memory(|memory| memory.focused());
        if let Some(index) = (0..items.len()).find(|index| focused == Some(id.with(("tab", index))))
        {
            for key in [
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::Home,
                egui::Key::End,
            ] {
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)) {
                    let next = match key {
                        egui::Key::ArrowLeft => (index + items.len() - 1) % items.len(),
                        egui::Key::ArrowRight => (index + 1) % items.len(),
                        egui::Key::Home => 0,
                        _ => items.len() - 1,
                    };
                    *selected = items[next].0;
                    changed = true;
                }
            }
        }
    }
    if changed {
        if let Some(index) = items.iter().position(|item| item.0 == *selected) {
            ui.memory_mut(|memory| memory.request_focus(id.with(("tab", index))));
        }
        ui.ctx().request_repaint();
    }
    changed
}

pub(crate) fn closable(
    ui: &mut Ui,
    salt: impl Hash + std::fmt::Debug,
    selected: &mut usize,
    items: &[(String, bool)],
) -> Option<usize> {
    let id = ui.make_persistent_id(&salt);
    ui.ctx()
        .data_mut(|d| d.insert_temp(id.with("closable"), true));
    show(
        ui,
        salt,
        selected,
        Style::Compact,
        items.iter().enumerate().map(|(i, (label, close))| {
            (
                i,
                if *close {
                    format!("{label}     ")
                } else {
                    label.clone()
                },
            )
        }),
    );
    let mut closed = None;
    for (index, (_, close)) in items.iter().enumerate() {
        if !close {
            continue;
        }
        if let Some(rect) = ui
            .ctx()
            .data(|d| d.get_temp::<Rect>(id.with(("tab-rect", index))))
        {
            let button =
                Rect::from_center_size(pos2(rect.right() - 13., rect.center().y), vec2(16., 16.));
            let response = ui.interact(button, id.with(("tab-close", index)), Sense::click());
            let color = if response.hovered() {
                Color32::BLACK
            } else {
                Color32::from_gray(153)
            };
            for sign in [-1., 1.] {
                ui.painter().line_segment(
                    [
                        button.center() + vec2(-3.5, -3.5 * sign),
                        button.center() + vec2(3.5, 3.5 * sign),
                    ],
                    Stroke::new(1., color),
                );
            }
            if response.clicked() {
                closed = Some(index);
            }
        }
    }
    closed
}
