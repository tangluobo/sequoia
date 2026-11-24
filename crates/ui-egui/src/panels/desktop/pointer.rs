use crate::tabs::desktop::DesktopView;
use egui::{Event, PointerButton, Pos2};
use tomato_services::remote_desktop::Input;

pub(super) fn handle_surface(
    view: &mut DesktopView,
    event: &Event,
    response: &egui::Response,
    point: impl Fn(Pos2) -> (u16, u16),
    send: &mut impl FnMut(Input),
) -> bool {
    // Floating scrollbars share the image's layer and rectangle. Use egui's
    // interaction owner as well as geometry; a scrollbar press is never remote.
    let owns_event = match event {
        Event::PointerButton {
            pos, pressed: true, ..
        } => {
            response.ctx.pointer_target_at(*pos) == Some(response.id)
                && (!response.ctx.egui_is_using_pointer() || response.is_pointer_button_down_on())
        }
        Event::PointerMoved(_) => response.hovered(),
        _ => false,
    };
    handle(
        view,
        event,
        point,
        |pos| {
            owns_event
                && response.enabled()
                && response.interact_rect.contains(pos)
                && response.ctx.layer_id_at(pos) == Some(response.layer_id)
        },
        send,
    )
}

pub(super) fn motion(view: &mut DesktopView, (x, y): (u16, u16), send: &mut impl FnMut(Input)) {
    if view.position != Some((x, y)) {
        send(Input::Pointer {
            x,
            y,
            button: None,
            down: false,
            wheel: 0,
        });
        view.position = Some((x, y));
    }
}

/// Return whether an accepted press should focus the remote surface. Capture
/// belongs to buttons pressed on this surface, not to its final frame hover.
pub(super) fn handle(
    view: &mut DesktopView,
    event: &Event,
    point: impl Fn(Pos2) -> (u16, u16),
    accepts: impl Fn(Pos2) -> bool,
    send: &mut impl FnMut(Input),
) -> bool {
    match *event {
        Event::PointerMoved(pos) if view.buttons != 0 || accepts(pos) => {
            motion(view, point(pos), send);
        }
        Event::PointerButton {
            pos,
            button,
            pressed,
            ..
        } => {
            let button = match button {
                PointerButton::Primary => 1,
                PointerButton::Secondary => 2,
                PointerButton::Middle => 3,
                _ => return false,
            };
            let mask = 1 << button;
            if if pressed {
                !accepts(pos)
            } else {
                view.buttons & mask == 0
            } {
                return false;
            }
            let (x, y) = point(pos);
            if pressed {
                view.buttons |= mask;
            } else {
                view.buttons &= !mask;
            }
            view.position = Some((x, y));
            send(Input::Pointer {
                x,
                y,
                button: Some(button),
                down: pressed,
                wheel: 0,
            });
            return pressed;
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(x: f32, pressed: bool) -> Event {
        Event::PointerButton {
            pos: egui::pos2(x, 10.),
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    fn dispatch(view: &mut DesktopView, events: &[Event]) -> Vec<(u16, Option<u8>, bool)> {
        let mut sent = Vec::new();
        for event in events {
            handle(
                view,
                event,
                |p| (p.x.clamp(0., 99.) as u16, p.y as u16),
                |p| (0.0..100.).contains(&p.x),
                &mut |input| sent.push(input),
            );
        }
        view.queue_inputs(sent);
        view.pending
            .drain(..)
            .map(|input| match input {
                Input::Pointer {
                    x, button, down, ..
                } => (x, button, down),
                _ => panic!("unexpected keyboard event"),
            })
            .collect()
    }

    #[test]
    fn fast_drag_in_one_frame_keeps_press_motion_release_order_and_last_position() {
        let mut view = DesktopView::default();
        assert_eq!(
            dispatch(
                &mut view,
                &[
                    button(10., true),
                    Event::PointerMoved(egui::pos2(30., 10.)),
                    Event::PointerMoved(egui::pos2(60., 10.)),
                    button(60., false),
                ]
            ),
            vec![(10, Some(1), true), (60, None, false), (60, Some(1), false)]
        );
        assert_eq!(view.position, Some((60, 10)));
        assert_eq!(view.buttons, 0);
    }

    #[test]
    fn drag_captures_motion_and_release_outside_surface_without_capturing_local_clicks() {
        let mut view = DesktopView::default();
        assert_eq!(
            dispatch(&mut view, &[button(10., true)]),
            vec![(10, Some(1), true)]
        );
        assert_eq!(
            dispatch(
                &mut view,
                &[
                    Event::PointerMoved(egui::pos2(120., 10.)),
                    button(120., false),
                    Event::PointerMoved(egui::pos2(130., 10.)),
                    button(130., true),
                    button(130., false),
                    button(20., false),
                ]
            ),
            vec![(99, None, false), (99, Some(1), false)]
        );
        assert_eq!(view.buttons, 0);
        assert_eq!(view.position, Some((99, 10)));
    }

    fn surface_frame(
        ctx: &egui::Context,
        view: &mut DesktopView,
        events: Vec<Event>,
    ) -> egui::Vec2 {
        let mut offset = egui::Vec2::ZERO;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(300., 200.),
                )),
                events: events.clone(),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(root, |ui| {
                        let output = egui::ScrollArea::both()
                            .id_salt("remote-pointer-test")
                            .auto_shrink([false, false])
                            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
                            .show(ui, |ui| {
                                ui.allocate_exact_size(
                                    egui::vec2(600., 400.),
                                    egui::Sense::click_and_drag(),
                                )
                            });
                        offset = output.state.offset;
                        let (rect, response) = output.inner;
                        let mut outgoing = Vec::new();
                        for event in &events {
                            handle_surface(
                                view,
                                event,
                                &response,
                                |p| {
                                    (
                                        (p.x - rect.min.x).clamp(0., 599.) as u16,
                                        (p.y - rect.min.y).clamp(0., 399.) as u16,
                                    )
                                },
                                &mut |input| outgoing.push(input),
                            );
                        }
                        view.queue_inputs(outgoing);
                    });
            },
        );
        output.textures_delta.clear();
        offset
    }

    #[test]
    fn actual_scrollbar_drag_does_not_press_the_remote_image_beneath_it() {
        let ctx = egui::Context::default();
        let mut view = DesktopView::default();
        for _ in 0..3 {
            surface_frame(&ctx, &mut view, vec![]);
        }
        let press = Event::PointerButton {
            pos: egui::pos2(299., 30.),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        };
        surface_frame(
            &ctx,
            &mut view,
            vec![Event::PointerMoved(egui::pos2(299., 30.)), press],
        );
        let offset = surface_frame(
            &ctx,
            &mut view,
            vec![Event::PointerMoved(egui::pos2(200., 70.))],
        );
        assert!(offset.y > 0., "the native scrollbar must actually scroll");
        assert_eq!(view.buttons, 0);
        assert!(
            view.pending.iter().all(|input| !matches!(
                input,
                Input::Pointer {
                    button: Some(_),
                    ..
                }
            )),
            "scrollbar input reached the remote desktop"
        );
    }

    #[test]
    fn actual_surface_fast_press_move_release_retains_drag_order() {
        let ctx = egui::Context::default();
        let mut view = DesktopView::default();
        for _ in 0..3 {
            surface_frame(&ctx, &mut view, vec![]);
        }
        surface_frame(
            &ctx,
            &mut view,
            vec![
                Event::PointerMoved(egui::pos2(10., 10.)),
                button(10., true),
                Event::PointerMoved(egui::pos2(60., 10.)),
                button(60., false),
            ],
        );
        let buttons: Vec<_> = view
            .pending
            .iter()
            .filter_map(|input| match input {
                Input::Pointer {
                    x,
                    button: Some(1),
                    down,
                    ..
                } => Some((*x, *down)),
                _ => None,
            })
            .collect();
        assert_eq!(buttons, vec![(10, true), (60, false)]);
        let press = view
            .pending
            .iter()
            .position(|input| {
                matches!(
                    input,
                    Input::Pointer {
                        button: Some(1),
                        down: true,
                        ..
                    }
                )
            })
            .unwrap();
        let movement = view
            .pending
            .iter()
            .position(|input| {
                matches!(
                    input,
                    Input::Pointer {
                        x: 60,
                        button: None,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(press < movement);
        assert_eq!(view.buttons, 0);
    }
}
