use super::*;
use tomato_services::json_history::Action;
pub(super) fn render(ui: &mut Ui, tool: &mut ToolState, busy: bool) -> Option<Command> {
    let mut action = None;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("历史记录")
                        .size(13.)
                        .strong()
                        .color(Color32::from_gray(85)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                RichText::new("清空").size(11.).color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(244, 67, 54)),
                        )
                        .clicked()
                        && !tool.history.is_empty()
                    {
                        action = Some(Action::Clear);
                    }
                });
            });
        });
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.;
        let id = ui.id().with("json-history-list");
        let list = ui.interact(ui.available_rect_before_wrap(), id, egui::Sense::click());
        let mut scroll_to = None;
        if list.has_focus() && !busy && !tool.history.is_empty() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
            for key in [
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::Home,
                egui::Key::End,
                egui::Key::PageUp,
                egui::Key::PageDown,
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    let current = tool
                        .history
                        .iter()
                        .position(|item| Some(&item.key.digest) == tool.history_selection.as_ref());
                    let last = tool.history.len() - 1;
                    let page = (ui.available_height() / 21.).floor().max(1.) as usize;
                    let row = match key {
                        egui::Key::Home => 0,
                        egui::Key::End => last,
                        egui::Key::ArrowDown => current.map_or(0, |i| (i + 1).min(last)),
                        egui::Key::ArrowUp => current.unwrap_or(0).saturating_sub(1),
                        egui::Key::PageDown => current.unwrap_or(0).saturating_add(page).min(last),
                        egui::Key::PageUp => current.unwrap_or(0).saturating_sub(page),
                        _ => unreachable!(),
                    };
                    tool.history_selection = Some(tool.history[row].key.digest.clone());
                    scroll_to = Some(row);
                }
            }
        }
        let width = tool
            .history
            .iter()
            .map(|item| {
                ui.fonts_mut(|fonts| {
                    fonts.layout_no_wrap(
                        format!("{}  {}", item.timestamp, item.summary),
                        egui::FontId::proportional(11.),
                        Color32::from_gray(51),
                    )
                })
                .size()
                .x + 16.
            })
            .fold(ui.available_width(), f32::max);
        let count = tool
            .history
            .len()
            .max((ui.available_height().max(0.) / 21.) as usize);
        let mut scroll = ScrollArea::both().id_salt(id).auto_shrink([false, false]);
        if let Some(row) = scroll_to {
            scroll = scroll
                .vertical_scroll_offset((row as f32 * 21. - ui.available_height() / 2.).max(0.));
        }
        scroll.show_rows(ui, 21., count, |ui, rows| {
            for row in rows {
                let item = tool.history.get(row);
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(width, 21.),
                    if !busy && item.is_some() {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                );
                if !busy
                    && response.is_pointer_button_down_on()
                    && ui.input(|i| i.pointer.any_pressed())
                    && let Some(item) = item
                {
                    tool.history_selection = Some(item.key.digest.clone());
                    list.request_focus();
                    ui.ctx().request_repaint();
                }
                let selected = item
                    .is_some_and(|item| tool.history_selection.as_ref() == Some(&item.key.digest));
                ui.painter().rect_filled(
                    rect,
                    0.,
                    if selected {
                        if list.has_focus() {
                            Color32::from_rgb(0, 150, 201)
                        } else {
                            Color32::from_gray(211)
                        }
                    } else if row % 2 == 0 {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(250)
                    },
                );
                let Some(item) = item else {
                    continue;
                };
                ui.painter().text(
                    egui::pos2(rect.left() + 8., rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{}  {}", item.timestamp, item.summary),
                    egui::FontId::proportional(11.),
                    if selected && list.has_focus() {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(51)
                    },
                );
                if response.clicked() || response.secondary_clicked() {
                    list.request_focus();
                    tool.history_selection = Some(item.key.digest.clone());
                    action = Some(Action::Restore(item.key.clone()));
                }
                response.context_menu(|ui| {
                    for (label, copy) in [("删除此记录", false), ("复制JSON", true)] {
                        if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                            action = Some(if copy {
                                Action::Copy(item.key.clone())
                            } else {
                                Action::Remove(item.key.clone())
                            });
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    action.map(|action| Command::JsonHistory {
        path: tool.history_path.clone(),
        action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_history_keyboard_selection_scrolls_without_restoring_and_keeps_original_menu_order() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = ToolState::new("json_tool");
        tool.history = (0..50)
            .map(|n| tomato_services::json_history::Item {
                key: tomato_services::json_history::Key {
                    index: n,
                    digest: n.to_string(),
                },
                timestamp: "00".into(),
                summary: format!("entry{n}"),
            })
            .collect();
        let frame = |tool: &mut ToolState, events| {
            let mut command = None;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260., 230.),
                    )),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        command = render(ui, tool, false);
                    });
                },
            );
            out.textures_delta.clear();
            (out, command)
        };
        let (out, _) = frame(&mut tool, vec![]);
        let row_point = |out: &egui::FullOutput, text: &str| {
            out.shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape
                        && t.galley.job.text == text
                    {
                        Some(t.pos + t.galley.size() / 2.)
                    } else {
                        None
                    }
                })
                .expect("history row visible")
        };
        let pos = row_point(&out, "00  entry0");
        frame(
            &mut tool,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let (_, command) = frame(
            &mut tool,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(matches!(
            command,
            Some(Command::JsonHistory {
                action: Action::Restore(_),
                ..
            })
        ));
        let (out, command) = frame(
            &mut tool,
            vec![egui::Event::Key {
                key: egui::Key::End,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(command.is_none());
        assert_eq!(tool.history_selection.as_deref(), Some("49"));
        let (out, _) = if out
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text=="00  entry49"))
        {
            (out, None)
        } else {
            frame(&mut tool, vec![])
        };
        let pos = row_point(&out, "00  entry49");
        frame(
            &mut tool,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &mut tool,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let (out, _) = frame(&mut tool, vec![]);
        let remove = row_point(&out, "删除此记录");
        let copy = row_point(&out, "复制JSON");
        assert!(remove.y < copy.y);
    }
}
