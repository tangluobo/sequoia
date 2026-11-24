//! Java ComboBox popup extends beyond the short native configuration window.
use super::*;
use egui::{Align2, Color32, Key, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
const NAMES: [&str; 13] = [
    "Sans Serif",
    "Serif",
    "Monospace",
    "Dialog",
    "DialogInput",
    "Arial",
    "Times New Roman",
    "Courier New",
    "Microsoft YaHei",
    "SimHei",
    "SimSun",
    "KaiTi",
    "FangSong",
];
pub(super) struct Picker {
    anchor: egui::Pos2,
    index: usize,
    focused: bool,
    reveal: bool,
}
fn viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("table-font-popup")
}
pub(super) fn close(ctx: &egui::Context) {
    ctx.send_viewport_cmd_to(viewport(), egui::ViewportCommand::Close);
}
pub(super) fn combo(ui: &mut Ui, rect: Rect, name: &mut String, picker: &mut Option<Picker>) {
    let id = ui.make_persistent_id("table-font-family");
    let response = ui.interact(rect, id, Sense::click());
    crate::native_dialog::paint_button(
        ui,
        rect,
        crate::native_dialog::ButtonState {
            default: false,
            hover: response.hovered(),
            armed: response.is_pointer_button_down_on(),
            focused: response.has_focus(),
            enabled: ui.is_enabled(),
        },
    );
    let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(6., 0.)));
    painter
        .with_clip_rect(Rect::from_min_max(rect.min, rect.max - vec2(18., 0.)))
        .text(
            rect.left_center() + vec2(8., 0.),
            Align2::LEFT_CENTER,
            &*name,
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::BLACK,
        );
    let p = pos2(rect.right() - 11., rect.center().y);
    painter.add(egui::Shape::convex_polygon(
        vec![p + vec2(-3., -1.5), p + vec2(3., -1.5), p + vec2(0., 1.5)],
        Color32::from_gray(70),
        Stroke::NONE,
    ));
    let toggle = response.clicked()
        || (response.has_focus()
            && ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                    || i.consume_key(egui::Modifiers::NONE, Key::Space)
            }));
    if toggle {
        response.request_focus();
        if picker.take().is_some() {
            close(ui.ctx());
        } else {
            let origin = ui.input(|i| i.viewport().inner_rect.map_or(egui::Pos2::ZERO, |r| r.min));
            *picker = Some(Picker {
                anchor: origin + rect.left_bottom().to_vec2(),
                index: NAMES.iter().position(|s| *s == name.as_str()).unwrap_or(0),
                focused: false,
                reveal: true,
            });
        }
    }
    if picker.is_some()
        && ui.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
        })
    {
        picker.take();
        close(ui.ctx());
    }
    let Some(current) = picker.as_mut() else {
        return;
    };
    let parent = ui.ctx().viewport_id();
    let mut selected = None;
    let mut dismissed = false;
    ui.ctx().show_viewport_immediate(
        viewport(),
        egui::ViewportBuilder::default()
            .with_title("表格字体")
            .with_inner_size([150., 232.])
            .with_position(current.anchor)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top(),
        |ui, _| {
            let bounds = ui.max_rect();
            ui.painter().rect(
                bounds,
                0.,
                Color32::WHITE,
                Stroke::new(1., Color32::from_gray(150)),
                StrokeKind::Inside,
            );
            let focused = ui.input(|i| i.focused);
            dismissed =
                ui.input(|i| i.viewport().close_requested()) || (current.focused && !focused);
            current.focused |= focused;
            for (key, next) in [
                (Key::ArrowDown, (current.index + 1).min(NAMES.len() - 1)),
                (Key::ArrowUp, current.index.saturating_sub(1)),
                (Key::Home, 0),
                (Key::End, NAMES.len() - 1),
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    current.index = next;
                    current.reveal = true;
                }
            }
            dismissed |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                selected = Some(current.index);
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(bounds.shrink(1.)), |ui| {
                ui.spacing_mut().item_spacing = vec2(0., 0.);
                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if std::mem::take(&mut current.reveal) {
                    area =
                        area.vertical_scroll_offset(current.index.saturating_sub(9) as f32 * 23.);
                }
                area.show_rows(ui, 23., NAMES.len(), |ui, rows| {
                    for (index, name) in NAMES.iter().enumerate().take(rows.end).skip(rows.start) {
                        let (r, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 23.), Sense::click());
                        let active = current.index == index;
                        let fill = if active {
                            Color32::from_rgb(53, 146, 203)
                        } else if response.hovered() {
                            Color32::from_rgb(216, 230, 242)
                        } else {
                            Color32::WHITE
                        };
                        ui.painter().rect_filled(r, 0., fill);
                        ui.painter()
                            .with_clip_rect(r.intersect(ui.clip_rect()))
                            .text(
                                r.left_center() + vec2(8., 0.),
                                Align2::LEFT_CENTER,
                                *name,
                                crate::native_dialog::font(ui.ctx(), 12.),
                                if active {
                                    Color32::WHITE
                                } else {
                                    Color32::BLACK
                                },
                            );
                        if response.clicked() {
                            selected = Some(index);
                        }
                    }
                });
            });
        },
    );
    if let Some(index) = selected {
        *name = NAMES[index].into();
    }
    if selected.is_some() || dismissed {
        picker.take();
        close(ui.ctx());
        ui.ctx()
            .send_viewport_cmd_to(parent, egui::ViewportCommand::Focus);
        ui.ctx().request_repaint();
    }
}
