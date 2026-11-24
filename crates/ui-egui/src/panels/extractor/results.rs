use crate::tabs::extractor::State;
use egui::{Align2, Color32, FontId, Key, Modifiers, ScrollArea, Sense, Ui};
const ROW: f32 = 21.;

pub(super) fn render(ui: &mut Ui, view: &mut State) {
    let rect = ui.available_rect_before_wrap();
    let id = ui.id().with("extractor-result-list");
    let response = ui.interact(rect, id, Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    let mut scroll = None;
    if response.has_focus() && !view.rows.is_empty() {
        let page = (rect.height() / ROW).floor().max(1.) as usize;
        for key in [
            Key::ArrowDown,
            Key::ArrowUp,
            Key::Home,
            Key::End,
            Key::PageDown,
            Key::PageUp,
        ] {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                let last = view.rows.len() - 1;
                let index = match key {
                    Key::Home => 0,
                    Key::End => last,
                    Key::ArrowDown => view.selected_row.map_or(0, |i| (i + 1).min(last)),
                    Key::ArrowUp => view.selected_row.unwrap_or(0).saturating_sub(1),
                    Key::PageDown => view
                        .selected_row
                        .unwrap_or(0)
                        .saturating_add(page)
                        .min(last),
                    Key::PageUp => view.selected_row.unwrap_or(0).saturating_sub(page),
                    _ => unreachable!(),
                };
                view.selected_row = Some(index);
                scroll = Some(index);
            }
        }
    }
    let mut area = ScrollArea::both().id_salt(id).auto_shrink([false, false]);
    if std::mem::take(&mut view.scroll_to_last) {
        area = area.vertical_scroll_offset(view.rows.len() as f32 * ROW);
    } else if let Some(index) = scroll {
        area = area.vertical_scroll_offset((index as f32 * ROW - rect.height() / 2.).max(0.));
    }
    let count = view
        .rows
        .len()
        .max((rect.height() / ROW).ceil().max(0.) as usize);
    ui.spacing_mut().item_spacing.y = 0.;
    let output = area.show_rows(ui, ROW, count, |ui, range| {
        for index in range {
            let text = view.rows.get(index);
            let galley = text.map(|text| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(
                        text.clone(),
                        FontId::proportional(12.),
                        Color32::from_gray(51),
                    )
                })
            });
            if let Some(galley) = &galley {
                view.rows_width = view.rows_width.max(galley.size().x + 18.);
            }
            let (row, hit) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(view.rows_width), ROW),
                Sense::CLICK,
            );
            let selected = text.is_some() && view.selected_row == Some(index);
            let background = if selected {
                if response.has_focus() {
                    Color32::from_rgb(0, 150, 201)
                } else {
                    Color32::from_gray(211)
                }
            } else if index % 2 == 1 {
                Color32::from_gray(248)
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(row, 0., background);
            if let Some(galley) = galley {
                ui.painter().galley_with_override_text_color(
                    egui::pos2(row.left() + 8., row.center().y - galley.size().y / 2.),
                    galley,
                    if selected && response.has_focus() {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(51)
                    },
                );
                if hit.clicked() || hit.secondary_clicked() {
                    view.selected_row = Some(index);
                    response.request_focus();
                }
            }
        }
    });
    if view.rows.is_empty() {
        ui.painter().text(
            output.inner_rect.center(),
            Align2::CENTER_CENTER,
            "等待运行...",
            FontId::proportional(12.),
            Color32::from_gray(51),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_results_scroll_to_end_select_whole_rows_and_follow_keyboard_navigation() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut view = State {
            rows: (0..500).map(|i| format!("result {i:03}")).collect(),
            scroll_to_last: true,
            ..Default::default()
        };
        let frame = |view: &mut State, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 210.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| render(ui, view),
            );
            let texts: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => {
                        Some((t.galley.job.text.clone(), t.pos + t.galley.size() / 2.))
                    }
                    _ => None,
                })
                .collect();
            output.textures_delta.clear();
            texts
        };
        frame(&mut view, vec![]);
        let texts = frame(&mut view, vec![]);
        assert!(
            texts.len() < 20,
            "only visible rows should create text shapes"
        );
        assert!(!texts.iter().any(|(text, _)| text == "result 000"));
        let mut point = texts
            .iter()
            .find(|(text, _)| text == "result 499")
            .expect("original list scrolls to last result")
            .1;
        point.x = 500.; // Clicking blank space in the row also selects it.
        frame(
            &mut view,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        frame(
            &mut view,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert_eq!(view.selected_row, Some(499));
        for (key, expected) in [
            (Key::Home, 0),
            (Key::ArrowDown, 1),
            (Key::End, 499),
            (Key::ArrowUp, 498),
        ] {
            frame(
                &mut view,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
            );
            assert_eq!(view.selected_row, Some(expected));
            frame(
                &mut view,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
            );
        }
        view.begin();
        assert!(view.rows.is_empty() && view.selected_row.is_none());
    }
}
