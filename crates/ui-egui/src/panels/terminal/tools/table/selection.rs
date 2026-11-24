//! Java RowSelectorDragSelection uses displayed rows, including filtered port lists.
use super::*;
use std::collections::BTreeSet;
pub(super) fn update(
    ui: &Ui,
    bounds: egui::Rect,
    visible: &[usize],
    selected: &mut BTreeSet<usize>,
    anchor: &mut Option<usize>,
    section: Section,
) {
    let id = ui.make_persistent_id("ssh-table-selector-drag");
    let (position, primary, secondary, down, modifiers, focused) = ui.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.primary_pressed(),
            i.pointer.secondary_pressed(),
            i.pointer.primary_down(),
            i.modifiers,
            i.focused,
        )
    });
    let row_at = |p: egui::Pos2| {
        let row = ((p.y - bounds.top() - 24.) / 24.).floor();
        (row >= 0. && (row as usize) < visible.len()).then_some(row as usize)
    };
    if ui.is_enabled()
        && ui.rect_contains_pointer(bounds)
        && let Some(p) = position
        && (primary || secondary)
        && p.y >= bounds.top() + 24.
    {
        if let Some(display) = row_at(p) {
            let index = visible[display];
            if modifiers.shift && !secondary {
                let start = anchor
                    .and_then(|index| visible.iter().position(|v| *v == index))
                    .unwrap_or(display);
                if !modifiers.ctrl {
                    selected.clear();
                }
                selected.extend(
                    visible[start.min(display)..=start.max(display)]
                        .iter()
                        .copied(),
                );
            } else {
                super::select(
                    selected,
                    anchor,
                    index,
                    if secondary {
                        egui::Modifiers::NONE
                    } else {
                        modifiers
                    },
                    secondary,
                );
            }
            if primary && p.x < bounds.left() + 15. && !modifiers.ctrl && !modifiers.shift {
                ui.data_mut(|d| d.insert_temp(id, display));
            }
        } else if primary || section == Section::Ports {
            selected.clear();
            *anchor = None;
        }
    }
    if let Some(start) = ui.data(|d| d.get_temp::<usize>(id)) {
        if !modifiers.ctrl
            && !modifiers.shift
            && !visible.is_empty()
            && let Some(p) = position
        {
            let target = ((p.y - bounds.top() - 24.) / 24.)
                .floor()
                .clamp(0., (visible.len() - 1) as f32) as usize;
            let start = start.min(visible.len() - 1);
            selected.clear();
            selected.extend(
                visible[start.min(target)..=start.max(target)]
                    .iter()
                    .copied(),
            );
        }
        if !down || !focused || position.is_none() {
            ui.data_mut(|d| d.remove::<usize>(id));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2};
    #[test]
    fn row_selector_drag_uses_filtered_rows_and_keeps_the_range_on_secondary_press() {
        let ctx = egui::Context::default();
        let mut selected = BTreeSet::new();
        let mut anchor = None;
        let mut frame = |events: Vec<Event>, modifiers| {
            let mut events = events;
            events.insert(0, Event::ModifiersChanged(modifiers));
            let mut output = ctx.run_ui(
                egui::RawInput {
                    focused: true,
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(400., 300.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let bounds =
                        egui::Rect::from_min_size(egui::pos2(8., 8.), egui::vec2(380., 240.));
                    ui.allocate_rect(bounds, egui::Sense::click());
                    update(
                        ui,
                        bounds,
                        &[0, 2, 4],
                        &mut selected,
                        &mut anchor,
                        Section::Ports,
                    );
                },
            );
            output.textures_delta.clear();
            selected.clone()
        };
        let button = |pos, button, pressed, modifiers| {
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                },
            ]
        };
        let p = egui::pos2(12., 44.);
        frame(vec![Event::PointerMoved(p)], Modifiers::NONE);
        assert_eq!(
            frame(
                button(p, PointerButton::Primary, true, Modifiers::NONE),
                Modifiers::NONE
            ),
            [0].into()
        );
        let end = egui::pos2(60., 92.);
        assert_eq!(
            frame(vec![Event::PointerMoved(end)], Modifiers::NONE),
            [0, 2, 4].into()
        );
        assert_eq!(
            frame(
                button(end, PointerButton::Primary, false, Modifiers::NONE),
                Modifiers::NONE
            ),
            [0, 2, 4].into()
        );
        let middle = egui::pos2(60., 68.);
        assert_eq!(
            frame(
                button(middle, PointerButton::Secondary, true, Modifiers::NONE),
                Modifiers::NONE
            ),
            [0, 2, 4].into()
        );
        frame(
            button(middle, PointerButton::Secondary, false, Modifiers::NONE),
            Modifiers::NONE,
        );
        assert_eq!(
            frame(
                button(middle, PointerButton::Primary, true, Modifiers::CTRL),
                Modifiers::CTRL
            ),
            [0, 4].into()
        );
        frame(
            button(middle, PointerButton::Primary, false, Modifiers::CTRL),
            Modifiers::CTRL,
        );
        let blank = egui::pos2(60., 180.);
        assert!(
            frame(
                button(blank, PointerButton::Primary, true, Modifiers::NONE),
                Modifiers::NONE
            )
            .is_empty()
        );
    }
}
