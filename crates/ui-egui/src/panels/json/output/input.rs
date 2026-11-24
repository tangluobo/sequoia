use super::*;
use crate::tabs::json::selection as positions;
#[expect(clippy::too_many_arguments)]
pub(super) fn handle(
    ui: &mut Ui,
    text: &str,
    view: &mut JsonView,
    selection: &mut Selection,
    font: &FontId,
    height: f32,
    origin: Pos2,
    gutter: f32,
    response: &egui::Response,
    kind: Kind,
) {
    if !ui.is_enabled() {
        selection.dragging = false;
        return;
    }
    let now = ui.input(|i| i.time);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let point = |ui: &Ui, view: &mut JsonView, pos: Pos2| {
        if pos.y < origin.y {
            return Point::default();
        }
        if !view.visible.is_empty() && pos.y >= origin.y + height * view.visible.len() as f32 {
            let row = view.visible.len() - 1;
            return Point {
                row,
                byte: view.visible_line(text, row).len(),
            };
        }
        let row = (((pos.y - origin.y) / height).floor().max(0.) as usize)
            .min(view.visible.len().saturating_sub(1));
        let data = view.visible_line(text, row);
        let byte = view.visible.get(row).copied().map_or(0, |line| {
            metrics::byte(ui, &data, view, font, line, pos.x - origin.x - gutter)
        });
        Point { row, byte }
    };
    if response.is_pointer_button_down_on()
        && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
        && let Some(pos) = pointer
    {
        response.request_focus();
        let hit = point(ui, view, pos);
        let data = view.visible_line(text, hit.row);
        selection.press(
            hit,
            &data,
            ui.input(|i| i.modifiers.shift),
            now,
            pos,
            hit.row + 1 < view.visible.len(),
        );
    } else if selection.dragging
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pos) = pointer
    {
        let hit = point(ui, view, pos);
        let data = view.visible_line(text, hit.row);
        selection.drag_to(hit, &data, hit.row + 1 < view.visible.len());
        let clip = response.rect;
        let clamp = |value: f32, min: f32, max: f32| {
            if value < min {
                (min - value).min(48.)
            } else if value > max {
                (max - value).max(-48.)
            } else {
                0.
            }
        };
        let delta = egui::vec2(
            clamp(pos.x, clip.left(), clip.right()),
            clamp(pos.y, clip.top(), clip.bottom()),
        );
        if delta != egui::Vec2::ZERO {
            ui.scroll_with_delta_animation(delta, egui::style::ScrollAnimation::none());
            ui.ctx().request_repaint();
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        selection.dragging = false;
    }
    if response.secondary_clicked() {
        response.request_focus();
    }
    let mut copy = false;
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        let events=ui.input_mut(|input|{
            let mut events=Vec::new();
            input.events.retain(|event|{
                let handled=matches!(event,egui::Event::Copy)||matches!(event,egui::Event::Key{key,pressed:true,modifiers,..}if matches!(key,Key::ArrowLeft|Key::ArrowRight|Key::ArrowUp|Key::ArrowDown|Key::Home|Key::End|Key::PageUp|Key::PageDown)||(modifiers.command || modifiers.ctrl)&&matches!(key,Key::A|Key::C));
                if handled{events.push(event.clone());} !handled
            });events
        });
        for event in events {
            match event {
                egui::Event::Copy => copy = true,
                egui::Event::Key {
                    key: Key::C,
                    modifiers,
                    ..
                } if (modifiers.command || modifiers.ctrl) => copy = true,
                egui::Event::Key {
                    key: Key::A,
                    modifiers,
                    ..
                } if (modifiers.command || modifiers.ctrl) => select_all(view, text, selection),
                egui::Event::Key { key, modifiers, .. } => {
                    if view.visible.is_empty() {
                        continue;
                    }
                    let mut hit = selection.head;
                    hit.row = hit.row.min(view.visible.len() - 1);
                    let data = view.visible_line(text, hit.row);
                    hit.byte = positions::floor(&data, hit.byte);
                    let vertical = matches!(
                        key,
                        Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown
                    );
                    let mut preferred = selection.preferred_x;
                    if vertical {
                        let x = *preferred.get_or_insert_with(|| {
                            f64::from(metrics::x(
                                ui,
                                &data,
                                view,
                                font,
                                view.visible[hit.row],
                                hit.byte,
                            ))
                        });
                        let step = if matches!(key, Key::PageUp | Key::PageDown) {
                            (ui.clip_rect().height() / height).floor().max(1.) as usize
                        } else {
                            1
                        };
                        hit.row = if matches!(key, Key::ArrowUp | Key::PageUp) {
                            hit.row.saturating_sub(step)
                        } else {
                            hit.row.saturating_add(step).min(view.visible.len() - 1)
                        };
                        let data = view.visible_line(text, hit.row);
                        hit.byte =
                            metrics::byte(ui, &data, view, font, view.visible[hit.row], x as f32);
                    } else if !modifiers.shift
                        && selection.range().is_some()
                        && matches!(key, Key::ArrowLeft | Key::ArrowRight)
                    {
                        let (start, end) = selection.range().unwrap();
                        hit = if key == Key::ArrowLeft { start } else { end };
                    } else {
                        match key {
                            Key::Home => {
                                if modifiers.command || modifiers.ctrl {
                                    hit.row = 0;
                                }
                                hit.byte = 0;
                            }
                            Key::End => {
                                if modifiers.command || modifiers.ctrl {
                                    hit.row = view.visible.len() - 1;
                                }
                                hit.byte = view.visible_line(text, hit.row).len();
                            }
                            Key::ArrowLeft => {
                                if hit.byte > 0 {
                                    hit.byte = if modifiers.command || modifiers.ctrl {
                                        positions::previous_word(&data, hit.byte)
                                    } else {
                                        positions::previous(&data, hit.byte)
                                    };
                                } else if hit.row > 0 {
                                    hit.row -= 1;
                                    hit.byte = view.visible_line(text, hit.row).len();
                                }
                            }
                            Key::ArrowRight => {
                                if hit.byte < data.len() {
                                    hit.byte = if modifiers.command || modifiers.ctrl {
                                        match kind {
                                            Kind::Rich => positions::next_word(&data, hit.byte),
                                            Kind::Raw => {
                                                positions::next_word_start(&data, hit.byte)
                                            }
                                        }
                                    } else {
                                        positions::next(&data, hit.byte)
                                    };
                                } else if hit.row + 1 < view.visible.len() {
                                    hit.row += 1;
                                    hit.byte = 0;
                                }
                            }
                            _ => {}
                        }
                    }
                    selection.move_to(hit, modifiers.shift);
                    if vertical {
                        selection.preferred_x = preferred;
                    }
                    selection.blink_start = now;
                    let data = view.visible_line(text, hit.row);
                    let x = metrics::x(ui, &data, view, font, view.visible[hit.row], hit.byte);
                    let pos = origin + egui::vec2(gutter + x, hit.row as f32 * height);
                    ui.scroll_to_rect_animation(
                        Rect::from_min_max(
                            pos - egui::vec2(gutter, 0.),
                            pos + egui::vec2(2., height),
                        ),
                        None,
                        egui::style::ScrollAnimation::none(),
                    );
                }
                _ => {}
            }
        }
    }
    response.context_menu(|ui| {
        if ui
            .add_enabled(selection.range().is_some(), egui::Button::new("复制"))
            .clicked()
        {
            copy = true;
            ui.close();
        }
        if ui
            .add_enabled(!view.visible.is_empty(), egui::Button::new("全选"))
            .clicked()
        {
            select_all(view, text, selection);
            ui.close();
        }
    });
    if copy && let Some(value) = view.selected_text(text, selection) {
        ui.ctx().copy_text(value);
    }
}
fn select_all(view: &JsonView, text: &str, selection: &mut Selection) {
    let row = view.visible.len().saturating_sub(1);
    selection.all(Point {
        row,
        byte: view.visible_line(text, row).len(),
    });
}
