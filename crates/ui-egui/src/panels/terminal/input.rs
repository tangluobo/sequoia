pub(super) fn paste(text: String, bracketed: bool) -> String {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~")
    } else {
        text
    }
}
pub(super) fn has_control_key(events: &[egui::Event], key: egui::Key) -> bool {
    events.iter().any(|event| matches!(event, egui::Event::Key {
        key: pressed_key, pressed: true, modifiers, ..
    } if *pressed_key == key && modifiers.ctrl && !modifiers.shift && !modifiers.alt && !modifiers.mac_cmd))
}
pub(super) fn retain_focus(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }
}
pub(super) fn focus_terminal(
    ui: &egui::Ui,
    response: &egui::Response,
    view: &mut crate::tabs::terminal::TerminalViewState,
    ready: bool,
    standalone: bool,
) {
    let modal = ui.memory(|memory| {
        memory
            .top_modal_layer()
            .is_some_and(|layer| layer != response.layer_id)
    });
    if !ui.is_enabled() || !response.enabled() || modal {
        if ui.memory(|memory| memory.has_focus(response.id)) {
            response.surrender_focus();
        }
        view.focused_once = false;
        return;
    }
    if ready && !standalone && !view.focused_once {
        if !ui.memory(|m| m.has_focus(response.id)) {
            response.request_focus();
        }
        // egui only installs a focus lock after a widget held focus in the
        // preceding frame. Keep activation pending until that lock can stick.
        view.focused_once =
            response.has_focus() && ui.memory(|m| m.had_focus_last_frame(response.id));
        if !view.focused_once && ui.input(|i| i.focused) {
            ui.ctx().request_repaint();
        }
    }
    retain_focus(ui, response);
}

pub(super) fn terminal_key(
    key: egui::Key,
    modifiers: egui::Modifiers,
    application: bool,
) -> Option<String> {
    use egui::Key;
    if modifiers.ctrl && !modifiers.alt {
        let letters = [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
        ];
        if let Some(index) = letters.iter().position(|value| *value == key) {
            return Some(char::from(index as u8 + 1).to_string());
        }
        match key {
            Key::Space => return Some("\0".into()),
            Key::Backslash => return Some("\u{1c}".into()),
            _ => {}
        }
    }
    if matches!(
        key,
        Key::ArrowUp | Key::ArrowDown | Key::ArrowRight | Key::ArrowLeft | Key::Home | Key::End
    ) {
        let code = match key {
            Key::ArrowUp => 'A',
            Key::ArrowDown => 'B',
            Key::ArrowRight => 'C',
            Key::ArrowLeft => 'D',
            Key::Home => 'H',
            _ => 'F',
        };
        // TerminalView dispatches special keys before its Control shortcuts.
        return Some(format!(
            "\u{1b}{}{code}",
            if application { "O" } else { "[" }
        ));
    }
    Some(
        match key {
            Key::Enter => "\r",
            Key::Backspace => "\u{8}",
            Key::Tab => "\t",
            Key::Escape => "\u{1b}",
            Key::ArrowUp => "\u{1b}[A",
            Key::ArrowDown => "\u{1b}[B",
            Key::ArrowRight => "\u{1b}[C",
            Key::ArrowLeft => "\u{1b}[D",
            Key::Home => "\u{1b}[H",
            Key::End => "\u{1b}[F",
            Key::Delete => "\u{1b}[3~",
            Key::PageUp => "\u{1b}[5~",
            Key::PageDown => "\u{1b}[6~",
            Key::Insert => "\u{1b}[2~",
            Key::F1 => "\u{1b}OP",
            Key::F2 => "\u{1b}OQ",
            Key::F3 => "\u{1b}OR",
            Key::F4 => "\u{1b}OS",
            Key::F5 => "\u{1b}[15~",
            Key::F6 => "\u{1b}[17~",
            Key::F7 => "\u{1b}[18~",
            Key::F8 => "\u{1b}[19~",
            Key::F9 => "\u{1b}[20~",
            Key::F10 => "\u{1b}[21~",
            Key::F11 => "\u{1b}[23~",
            Key::F12 => "\u{1b}[24~",
            _ => return None,
        }
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modal_password_keeps_input_while_the_background_terminal_is_ready() {
        let context = egui::Context::default();
        let id = egui::Id::new("background-terminal");
        let mut view = crate::tabs::terminal::TerminalViewState::default();
        let mut password = String::new();
        let mut leaked = String::new();
        for phase in 0..6 {
            let mut out = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    focused: true,
                    events: if phase == 2 {
                        vec![egui::Event::Text("fixture-only".into())]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
                |ui| {
                    let mut background =
                        ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect()));
                    if phase == 1 || phase == 2 {
                        background.disable();
                    }
                    let response = background.interact(
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 500.)),
                        id,
                        egui::Sense::click(),
                    );
                    focus_terminal(&background, &response, &mut view, true, false);
                    if phase == 2 && response.has_focus() {
                        for event in ui.input(|i| i.events.clone()) {
                            if let egui::Event::Text(text) = event {
                                leaked.push_str(&text);
                            }
                        }
                    }
                    if phase == 1 || phase == 2 {
                        egui::Modal::new(egui::Id::new("password-modal")).show(ui.ctx(), |ui| {
                            let field =
                                ui.add(egui::TextEdit::singleline(&mut password).password(true));
                            if phase == 1 {
                                field.request_focus();
                            }
                        });
                    }
                },
            );
            out.textures_delta.clear();
        }
        assert_eq!(password, "fixture-only");
        assert!(leaked.is_empty());
        assert!(context.memory(|m| m.has_focus(id)));
    }
    #[test]
    fn returning_to_terminal_tabs_recovers_focus_and_tab_while_standalone_files_keep_their_field() {
        let context = egui::Context::default();
        let mut views = [
            crate::tabs::terminal::TerminalViewState::default(),
            crate::tabs::terminal::TerminalViewState::default(),
        ];
        let ids = [
            egui::Id::new("first-terminal"),
            egui::Id::new("second-terminal"),
        ];
        let path_id = egui::Id::new("sftp-path");
        let mut path = String::new();
        let mut sent = String::new();
        for (step, active) in [Some(0), Some(1), Some(0), Some(0), None]
            .into_iter()
            .enumerate()
        {
            for (index, view) in views.iter_mut().enumerate() {
                if active != Some(index) {
                    view.deactivate();
                }
            }
            let events = if step == 3 {
                vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                Vec::new()
            };
            let mut output = context.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    if let Some(index) = active {
                        let response = ui.interact(
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 500.)),
                            ids[index],
                            egui::Sense::click(),
                        );
                        focus_terminal(ui, &response, &mut views[index], true, false);
                        assert!(
                            response.has_focus(),
                            "activation step {step}, focused={:?}",
                            context.memory(|m| m.focused())
                        );
                        if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                            sent.push_str(
                                &terminal_key(egui::Key::Tab, egui::Modifiers::NONE, false)
                                    .unwrap(),
                            );
                        }
                    } else {
                        let field = ui.add(egui::TextEdit::singleline(&mut path).id(path_id));
                        field.request_focus();
                        let response = ui.interact(
                            egui::Rect::from_min_size(egui::pos2(700., 0.), egui::vec2(0., 500.)),
                            egui::Id::new("standalone-files-terminal"),
                            egui::Sense::click(),
                        );
                        focus_terminal(ui, &response, &mut views[0], true, true);
                        assert!(field.has_focus());
                    }
                },
            );
            output.textures_delta.clear();
            if step == 0 {
                views[0].composition = "未完成的输入".into();
            }
            if step == 1 {
                assert!(views[0].composition.is_empty());
            }
            assert_eq!(
                context.memory(|m| m.focused()),
                Some(active.map_or(path_id, |index| ids[index]))
            );
        }
        assert_eq!(sent, "\t");
    }
    #[test]
    fn tab_stays_in_the_terminal_instead_of_focusing_the_next_control() {
        let context = egui::Context::default();
        let id = egui::Id::new("terminal-keyboard");
        for frame in 0..3 {
            let mut input = egui::RawInput {
                time: Some(f64::from(frame)),
                ..Default::default()
            };
            if frame == 2 {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
            let mut output = context.run_ui(input, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(0., 0.), egui::vec2(300., 200.)),
                    id,
                    egui::Sense::click_and_drag(),
                );
                if frame == 0 {
                    response.request_focus();
                }
                retain_focus(ui, &response);
                let _ = ui.button("下一个控件");
                if frame == 2 {
                    assert!(ui.input(|input| input.key_pressed(egui::Key::Tab)));
                    assert!(response.has_focus());
                }
            });
            output.textures_delta.clear();
        }
        assert!(context.memory(|memory| memory.has_focus(id)));
    }
    #[test]
    fn unshifted_clipboard_keys_still_send_terminal_control_codes() {
        assert_eq!(
            terminal_key(egui::Key::C, egui::Modifiers::CTRL, false).as_deref(),
            Some("\x03")
        );
        assert_eq!(
            terminal_key(egui::Key::V, egui::Modifiers::CTRL, false).as_deref(),
            Some("\x16")
        );
    }
    #[test]
    fn terminal_keys_respect_application_mode_and_control_sequences() {
        assert_eq!(
            terminal_key(egui::Key::Tab, Default::default(), false).as_deref(),
            Some("\t")
        );
        assert_eq!(
            terminal_key(egui::Key::Tab, egui::Modifiers::SHIFT, false).as_deref(),
            Some("\t")
        );
        assert_eq!(
            terminal_key(egui::Key::Backspace, Default::default(), false).as_deref(),
            Some("\x08")
        );
        assert_eq!(
            terminal_key(egui::Key::ArrowUp, Default::default(), false).as_deref(),
            Some("\x1b[A")
        );
        assert_eq!(
            terminal_key(egui::Key::ArrowUp, Default::default(), true).as_deref(),
            Some("\x1bOA")
        );
        assert_eq!(
            terminal_key(
                egui::Key::R,
                egui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false
            )
            .as_deref(),
            Some("\x12")
        );
        assert_eq!(
            terminal_key(egui::Key::F12, Default::default(), false).as_deref(),
            Some("\x1b[24~")
        );
    }
    #[test]
    fn modified_special_keys_follow_original_dispatch_order() {
        for modifiers in [
            egui::Modifiers::CTRL,
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            egui::Modifiers::ALT,
            egui::Modifiers::CTRL | egui::Modifiers::ALT,
        ] {
            for (key, expected) in [
                (egui::Key::Tab, "\t"),
                (egui::Key::Backspace, "\x08"),
                (egui::Key::Enter, "\r"),
                (egui::Key::Escape, "\x1b"),
                (egui::Key::Delete, "\x1b[3~"),
                (egui::Key::ArrowLeft, "\x1b[D"),
                (egui::Key::Home, "\x1b[H"),
            ] {
                assert_eq!(
                    terminal_key(key, modifiers, false).as_deref(),
                    Some(expected)
                );
            }
            assert_eq!(
                terminal_key(egui::Key::ArrowUp, modifiers, true).as_deref(),
                Some("\x1bOA")
            );
        }
    }
    #[test]
    fn paste_normalizes_line_endings_and_preserves_bracketed_mode() {
        assert_eq!(paste("a\r\nb\nc\r".into(), false), "a\rb\rc\r");
        assert_eq!(paste("a\r\nb\n".into(), true), "\x1b[200~a\rb\r\x1b[201~");
    }
    #[test]
    fn clipboard_control_shortcuts_use_event_time_modifiers() {
        let events = [
            egui::Event::Key {
                key: egui::Key::V,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            },
            egui::Event::Paste("clipboard".into()),
            egui::Event::Key {
                key: egui::Key::V,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            },
        ];
        assert!(has_control_key(&events, egui::Key::V));
        assert!(!has_control_key(&events, egui::Key::C));
        assert!(!has_control_key(&events[1..], egui::Key::V));
        // The extra raw key must not interfere with normal editor clipboard input.
        let context = egui::Context::default();
        let id = egui::Id::new("ordinary-editor-clipboard");
        let mut text = String::new();
        let mut output = context.run_ui(Default::default(), |ui| {
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .show(ui)
                .response
                .request_focus();
        });
        output.textures_delta.clear();
        let mut output = context.run_ui(
            egui::RawInput {
                events: events.to_vec(),
                ..Default::default()
            },
            |ui| {
                egui::TextEdit::singleline(&mut text).id(id).show(ui);
            },
        );
        output.textures_delta.clear();
        assert_eq!(text, "clipboard");
    }
}
