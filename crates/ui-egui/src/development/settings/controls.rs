//! Modena controls shared by the original project, SDK and build-tool dialogs.
use super::*;
use egui::{Color32, Rect, Sense, Stroke, pos2};
pub fn font(ui: &Ui, size: f32) -> egui::FontId {
    crate::native_dialog::font(ui.ctx(), size)
}
pub fn label(ui: &Ui, at: egui::Pos2, text: &str) {
    ui.painter().text(
        at,
        egui::Align2::LEFT_TOP,
        text,
        font(ui, 12.),
        Color32::from_gray(51),
    );
}
pub fn button(ui: &mut Ui, rect: Rect, text: &str, default: bool, enabled: bool) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let response = ui.interact(
        rect,
        ui.id().with(text),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    crate::native_dialog::paint_button(
        ui,
        rect,
        crate::native_dialog::ButtonState {
            default,
            hover: response.hovered(),
            armed: response.is_pointer_button_down_on(),
            focused: response.has_focus(),
            enabled,
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font(ui, 12.),
        Color32::from_gray(51).gamma_multiply(if enabled { 1. } else { 0.4 }),
    );
    if response.clicked() {
        response.request_focus();
    }
    response
}
pub fn footer(ui: &mut Ui, text: &str, enabled: bool, focus: &mut bool) -> (bool, bool) {
    let area = ui.max_rect();
    let accept = button(
        ui,
        Rect::from_min_size(area.right_bottom() - vec2(170., 33.), vec2(75., 24.)),
        text,
        true,
        enabled,
    );
    let cancel = button(
        ui,
        Rect::from_min_size(area.right_bottom() - vec2(85., 33.), vec2(75., 24.)),
        "取消",
        false,
        enabled,
    );
    if std::mem::take(focus) {
        accept.request_focus();
        ui.ctx().request_repaint();
    }
    let enter = enabled
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    (accept.clicked() || enter, cancel.clicked())
}
pub fn tabs(ui: &mut Ui, selected: &mut usize) {
    let origin = ui.max_rect().min;
    let background = Rect::from_min_size(
        origin + vec2(10., 10.),
        vec2(ui.max_rect().width() - 20., 29.),
    );
    ui.painter()
        .rect_filled(background, 0, Color32::from_gray(222));
    ui.painter().hline(
        background.x_range(),
        background.top() + 0.5,
        Stroke::new(1., Color32::from_gray(200)),
    );
    ui.painter().hline(
        background.x_range(),
        background.bottom() - 0.5,
        Stroke::new(1., Color32::from_gray(184)),
    );
    let mut x = origin.x + 15.;
    let mut focused = false;
    for (index, (text, width)) in [("项目配置", 60.), ("SDK 管理", 64.), ("构建管理", 60.)]
        .into_iter()
        .enumerate()
    {
        let active = *selected == index;
        let rect = Rect::from_min_max(
            pos2(x, origin.y + if active { 15. } else { 16. }),
            pos2(x + width, origin.y + 39.),
        );
        let response = ui.interact(rect, ui.id().with(("settings-tab", index)), Sense::click());
        focused |= response.has_focus();
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            Color32::from_gray(if active {
                244
            } else if response.hovered() {
                232
            } else {
                224
            }),
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            Stroke::new(1., Color32::from_gray(184)),
            egui::StrokeKind::Inside,
        );
        if active {
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1., Color32::from_gray(244)),
            );
        }
        ui.painter().text(
            pos2(x + 6., origin.y + 27.5),
            egui::Align2::LEFT_CENTER,
            text,
            font(ui, 12.),
            Color32::from_gray(51),
        );
        if response.clicked() {
            *selected = index;
            response.request_focus();
        }
        x += width;
    }
    let reverse = ui.input_mut(|i| {
        i.consume_key(
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            egui::Key::Tab,
        )
    });
    let forward = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Tab));
    if reverse
        || focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft))
    {
        *selected = (*selected + 2) % 3;
    }
    if forward
        || focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
    {
        *selected = (*selected + 1) % 3;
    }
}
pub fn combo(
    ui: &mut Ui,
    rect: Rect,
    key: &str,
    selected: &mut String,
    prompt: &str,
    values: &[(String, String)],
    enabled: bool,
) -> bool {
    let id = ui.id().with(key);
    let popup = id.with("popup");
    let enabled = enabled && ui.is_enabled();
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, prompt));
    let mut changed = false;
    if response.has_focus() && enabled {
        let open = ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::F4)
        });
        if open {
            egui::Popup::open_id(ui.ctx(), popup);
        }
        let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        if !values.is_empty() && (up || down) {
            let current = values.iter().position(|(id, _)| id == selected);
            let index = if up {
                current.unwrap_or(1).saturating_sub(1)
            } else {
                current.map_or(0, |i| (i + 1).min(values.len() - 1))
            };
            *selected = values[index].0.clone();
            changed = true;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            if egui::Popup::is_id_open(ui.ctx(), popup) {
                egui::Popup::close_id(ui.ctx(), popup);
            } else {
                egui::Popup::open_id(ui.ctx(), popup);
            }
        }
    }
    crate::native_dialog::paint_button(
        ui,
        rect,
        crate::native_dialog::ButtonState {
            default: false,
            hover: response.hovered(),
            armed: response.is_pointer_button_down_on(),
            focused: response.has_focus(),
            enabled,
        },
    );
    let value = values
        .iter()
        .find(|(id, _)| id == selected)
        .map(|(_, value)| value.as_str());
    ui.painter()
        .with_clip_rect(Rect::from_min_max(
            rect.min + vec2(2., 2.),
            rect.max - vec2(28., 2.),
        ))
        .text(
            rect.left_center() + vec2(8., 0.),
            egui::Align2::LEFT_CENTER,
            value.unwrap_or(prompt),
            font(ui, 12.),
            Color32::from_gray(if value.is_some() { 51 } else { 112 }).gamma_multiply(if enabled {
                1.
            } else {
                0.4
            }),
        );
    let center = rect.right_center() - vec2(11., 0.);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + vec2(-3.5, -1.),
            center + vec2(3.5, -1.),
            center + vec2(0., 2.5),
        ],
        Color32::from_gray(64).gamma_multiply(if enabled { 1. } else { 0.4 }),
        Stroke::NONE,
    ));
    if enabled {
        egui::Popup::menu(&response)
            .id(popup)
            .width(rect.width())
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1., Color32::from_gray(128))),
            )
            .show(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        for (id, value) in values {
                            let (row, hit) =
                                ui.allocate_exact_size(vec2(rect.width(), 24.), Sense::click());
                            let active = hit.hovered() || id == selected;
                            ui.painter().rect_filled(
                                row,
                                0,
                                if active {
                                    Color32::from_rgb(0, 150, 201)
                                } else {
                                    Color32::WHITE
                                },
                            );
                            ui.painter().text(
                                row.left_center() + vec2(6., 0.),
                                egui::Align2::LEFT_CENTER,
                                value,
                                font(ui, 12.),
                                if active {
                                    Color32::WHITE
                                } else {
                                    Color32::from_gray(51)
                                },
                            );
                            if hit.clicked() {
                                *selected = id.clone();
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            });
    }
    if response.clicked() {
        response.request_focus();
    }
    changed
}
pub fn field(
    ui: &mut Ui,
    rect: Rect,
    editor: &mut Editor,
    key: &str,
    value: &mut String,
    hint: &str,
    enabled: bool,
) -> bool {
    let id = editor.prepare(ui, key);
    let enabled = enabled && ui.is_enabled();
    let opacity = if enabled { 1. } else { 0.4 };
    ui.painter()
        .rect_filled(rect, 3, Color32::WHITE.gamma_multiply(opacity));
    ui.painter().rect_stroke(
        rect,
        3,
        Stroke::new(1., Color32::from_gray(200).gamma_multiply(opacity)),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    if !enabled {
        child.disable();
        child.set_opacity(opacity);
    }
    child.spacing_mut().interact_size.y = 23.;
    let response = egui::TextEdit::singleline(value)
        .id(id)
        .font(font(ui, 12.))
        .text_color(Color32::from_gray(51))
        .hint_text(egui::RichText::new(hint).color(Color32::from_gray(128)))
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 4)))
        .margin(vec2(8., 4.))
        .desired_width(rect.width())
        .min_size(vec2(0., 15.))
        .char_limit(32_768)
        .cursor_at_end(false)
        .show(&mut child)
        .response;
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            3,
            Stroke::new(1., Color32::from_rgb(0, 150, 201)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
        state.trim_undo_memory(512 * 1024);
        state.store(ui.ctx(), id);
    }
    response.changed()
}
