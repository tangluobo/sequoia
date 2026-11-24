use crate::tabs::desktop::DesktopView;
use egui::{Event, Key, Modifiers};
use tomato_services::remote_desktop::Input;

pub(super) fn release(view: &mut DesktopView, send: &mut impl FnMut(Input)) {
    view.release_inputs(send);
}
pub(super) fn modifiers(view: &mut DesktopView, next: Modifiers, send: &mut impl FnMut(Input)) {
    view.update_modifiers(next, send);
}
pub(super) fn handle(event: Event, view: &mut DesktopView, send: &mut impl FnMut(Input)) {
    match event {
        Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
            view.composition = text.chars().take(512).collect()
        }
        Event::Ime(egui::ImeEvent::DeleteSurrounding {
            before_chars,
            after_chars,
        }) => {
            for (count, code, extended) in [(before_chars, 0x0e, false), (after_chars, 0x53, true)]
            {
                for _ in 0..count.min(1024) {
                    for down in [true, false] {
                        send(Input::ScanCode {
                            code,
                            extended,
                            down,
                        });
                    }
                }
            }
        }
        Event::Key { .. } if !view.composition.is_empty() => {}
        Event::Key {
            key,
            physical_key,
            pressed,
            modifiers: mods,
            ..
        } => {
            modifiers(view, mods, send);
            let key = physical_key.unwrap_or(key);
            let Some((code, extended, printable)) = scan_code(key) else {
                return;
            };
            // Printable text arrives as Unicode; shortcuts use physical scan codes.
            if printable && !mods.ctrl && !mods.alt && !view.pressed.contains(&(code, extended)) {
                return;
            }
            if pressed && !view.pressed.contains(&(code, extended)) {
                view.pressed.push((code, extended));
            }
            if !pressed {
                view.pressed.retain(|key| *key != (code, extended));
            }
            send(Input::ScanCode {
                code,
                extended,
                down: pressed,
            });
        }
        Event::Paste(_) if view.protocol_clipboard => shortcut(0x2f, view, send),
        Event::Text(text) | Event::Paste(text) | Event::Ime(egui::ImeEvent::Commit(text)) => {
            view.composition.clear();
            for value in text.encode_utf16() {
                for down in [true, false] {
                    send(Input::Unicode { value, down });
                }
            }
        }
        Event::Copy | Event::Cut => {
            let code = if matches!(event, Event::Copy) {
                0x2e
            } else {
                0x2d
            };
            shortcut(code, view, send);
        }
        _ => {}
    }
}
fn shortcut(code: u8, view: &mut DesktopView, send: &mut impl FnMut(Input)) {
    // Our winit adapter preserves raw Ctrl+C/V as well as semantic clipboard
    // events. The physical press already reached RDP; a second shortcut would
    // paste files twice or restart a copy while its file list is being fetched.
    if view.pressed.contains(&(code, false)) {
        return;
    }
    let previous = view.modifiers;
    modifiers(
        view,
        Modifiers {
            ctrl: true,
            shift: previous.shift,
            ..Default::default()
        },
        send,
    );
    for down in [true, false] {
        send(Input::ScanCode {
            code,
            extended: false,
            down,
        });
    }
    modifiers(view, previous, send);
}
pub(super) fn scan_code(key: Key) -> Option<(u8, bool, bool)> {
    let (code, extended, printable) = match key {
        Key::Escape => (0x01, false, false),
        Key::Enter => (0x1c, false, false),
        Key::Tab => (0x0f, false, false),
        Key::Backspace => (0x0e, false, false),
        Key::ArrowUp => (0x48, true, false),
        Key::ArrowDown => (0x50, true, false),
        Key::ArrowLeft => (0x4b, true, false),
        Key::ArrowRight => (0x4d, true, false),
        Key::Home => (0x47, true, false),
        Key::End => (0x4f, true, false),
        Key::PageUp => (0x49, true, false),
        Key::PageDown => (0x51, true, false),
        Key::Insert => (0x52, true, false),
        Key::Delete => (0x53, true, false),
        Key::F1 => (0x3b, false, false),
        Key::F2 => (0x3c, false, false),
        Key::F3 => (0x3d, false, false),
        Key::F4 => (0x3e, false, false),
        Key::F5 => (0x3f, false, false),
        Key::F6 => (0x40, false, false),
        Key::F7 => (0x41, false, false),
        Key::F8 => (0x42, false, false),
        Key::F9 => (0x43, false, false),
        Key::F10 => (0x44, false, false),
        Key::F11 => (0x57, false, false),
        Key::F12 => (0x58, false, false),
        Key::A => (0x1e, false, true),
        Key::B => (0x30, false, true),
        Key::C => (0x2e, false, true),
        Key::D => (0x20, false, true),
        Key::E => (0x12, false, true),
        Key::F => (0x21, false, true),
        Key::G => (0x22, false, true),
        Key::H => (0x23, false, true),
        Key::I => (0x17, false, true),
        Key::J => (0x24, false, true),
        Key::K => (0x25, false, true),
        Key::L => (0x26, false, true),
        Key::M => (0x32, false, true),
        Key::N => (0x31, false, true),
        Key::O => (0x18, false, true),
        Key::P => (0x19, false, true),
        Key::Q => (0x10, false, true),
        Key::R => (0x13, false, true),
        Key::S => (0x1f, false, true),
        Key::T => (0x14, false, true),
        Key::U => (0x16, false, true),
        Key::V => (0x2f, false, true),
        Key::W => (0x11, false, true),
        Key::X => (0x2d, false, true),
        Key::Y => (0x15, false, true),
        Key::Z => (0x2c, false, true),
        Key::Space => (0x39, false, true),
        Key::Minus => (0x0c, false, true),
        Key::Equals => (0x0d, false, true),
        Key::OpenBracket => (0x1a, false, true),
        Key::CloseBracket => (0x1b, false, true),
        Key::Semicolon => (0x27, false, true),
        Key::Quote => (0x28, false, true),
        Key::Backtick => (0x29, false, true),
        Key::Backslash => (0x2b, false, true),
        Key::Comma => (0x33, false, true),
        Key::Period => (0x34, false, true),
        Key::Slash => (0x35, false, true),
        Key::Num0 => (0x0b, false, true),
        Key::Num1 => (0x02, false, true),
        Key::Num2 => (0x03, false, true),
        Key::Num3 => (0x04, false, true),
        Key::Num4 => (0x05, false, true),
        Key::Num5 => (0x06, false, true),
        Key::Num6 => (0x07, false, true),
        Key::Num7 => (0x08, false, true),
        Key::Num8 => (0x09, false, true),
        Key::Num9 => (0x0a, false, true),
        _ => return None,
    };
    Some((code, extended, printable))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_clipboard_shortcuts_send_one_physical_press_for_raw_and_semantic_events() {
        for (key, code, semantic) in [
            (Key::C, 0x2e, Event::Copy),
            (Key::V, 0x2f, Event::Paste("text".into())),
        ] {
            let mut view = DesktopView {
                protocol_clipboard: true,
                ..Default::default()
            };
            let mut sent = Vec::new();
            for event in [
                Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::CTRL,
                },
                semantic,
                Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::CTRL,
                },
            ] {
                handle(event, &mut view, &mut |input| sent.push(input));
            }
            assert_eq!(sent.iter().filter(|input| matches!(input, Input::ScanCode { code: value, down: true, .. } if *value == code)).count(), 1);
            assert_eq!(sent.iter().filter(|input| matches!(input, Input::ScanCode { code: value, down: false, .. } if *value == code)).count(), 1);
            assert!(
                !sent
                    .iter()
                    .any(|input| matches!(input, Input::Unicode { .. }))
            );
        }
    }
    #[test]
    fn protocol_paste_uses_remote_clipboard_and_restores_modifiers() {
        let mut view = DesktopView {
            protocol_clipboard: true,
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut events = Vec::new();
        handle(
            Event::Paste("中文".repeat(10_000)),
            &mut view,
            &mut |event| events.push(event),
        );
        assert_eq!(events.len(), 4);
        assert!(matches!(
            events[1],
            Input::ScanCode {
                code: 0x2f,
                down: true,
                ..
            }
        ));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Input::Unicode { .. }))
        );
        assert!(view.modifiers.shift && !view.modifiers.ctrl);
    }
    #[test]
    fn composition_commit_sends_unicode_without_the_candidate_enter_key() {
        let mut view = DesktopView::default();
        let mut events = Vec::new();
        let mut send = |event| events.push(event);
        handle(
            Event::Ime(egui::ImeEvent::Preedit {
                text: "zhong".into(),
                active_range_chars: None,
            }),
            &mut view,
            &mut send,
        );
        handle(
            Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            &mut view,
            &mut send,
        );
        handle(
            Event::Ime(egui::ImeEvent::Commit("中".into())),
            &mut view,
            &mut send,
        );
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            Input::Unicode {
                value: 0x4e2d,
                down: true
            }
        ));
        assert!(view.composition.is_empty());
    }
    #[test]
    fn unicode_does_not_duplicate_scancodes_and_focus_loss_releases_shortcuts() {
        let mut view = DesktopView::default();
        let mut events = Vec::new();
        let mut send = |event| events.push(event);
        handle(
            Event::Key {
                key: Key::A,
                physical_key: Some(Key::A),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            &mut view,
            &mut send,
        );
        handle(Event::Text("中".into()), &mut view, &mut send);
        handle(
            Event::Key {
                key: Key::A,
                physical_key: Some(Key::A),
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            },
            &mut view,
            &mut send,
        );
        release(&mut view, &mut send);
        assert!(matches!(
            events[0],
            Input::Unicode {
                value: 0x4e2d,
                down: true
            }
        ));
        assert_eq!(events.len(), 6);
        assert!(view.pressed.is_empty());
        assert!(!view.modifiers.ctrl);
    }
}
