//! Match physical key lifetimes with the OS-provided text, including Caps Lock
//! and alternate layouts. IME commits retain the shared Unicode text path.
use super::super::keyboard;
use crate::tabs::desktop::DesktopView;
use egui::{Event, Key};
use std::collections::VecDeque;
use tomato_services::remote_desktop::Input;

#[derive(Default)]
pub(crate) struct Typing {
    presses: VecDeque<(Key, u16)>,
    texts: usize,
}
impl Typing {
    pub(crate) fn new(events: &[Event], composing: bool) -> Self {
        if composing
            || events.iter().any(|event| {
                matches!(
                    event,
                    Event::Ime(egui::ImeEvent::Preedit { .. } | egui::ImeEvent::Commit(_))
                )
            })
        {
            return Self::default();
        }
        let keys = events
            .iter()
            .filter_map(|event| {
                if let Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.mac_cmd
                {
                    let key = physical_key.unwrap_or(*key);
                    if keyboard::scan_code(key).is_some_and(|(_, _, printable)| printable) {
                        return Some(key);
                    }
                }
                None
            })
            .collect::<Vec<_>>();
        let values = events
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) => Some(single_character(text)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let Some(values) = values else {
            return Self::default();
        };
        if keys.is_empty() || keys.len() != values.len() || keys.len() > 256 {
            return Self::default();
        }
        Self {
            texts: values.len(),
            presses: keys.into_iter().zip(values).collect(),
        }
    }
    pub(crate) fn handle(
        &mut self,
        event: &Event,
        view: &mut DesktopView,
        send: &mut impl FnMut(Input),
    ) -> bool {
        match event {
            Event::Key {
                key,
                physical_key,
                pressed,
                modifiers,
                ..
            } => {
                let key = physical_key.unwrap_or(*key);
                if !pressed {
                    if let Some(index) = view
                        .printable_pressed
                        .iter()
                        .position(|(held, _)| *held == key)
                    {
                        keyboard::modifiers(view, *modifiers, send);
                        let (_, value) = view.printable_pressed.remove(index);
                        send(Input::Unicode { value, down: false });
                        return true;
                    }
                } else if self.presses.front().is_some_and(|(next, _)| *next == key) {
                    let (_, value) = self.presses.pop_front().expect("checked front");
                    keyboard::modifiers(view, *modifiers, send);
                    let physical_held =
                        keyboard::scan_code(key).is_some_and(|(code, extended, _)| {
                            view.pressed.contains(&(code, extended))
                        });
                    if !physical_held
                        && !view.printable_pressed.iter().any(|(held, _)| *held == key)
                    {
                        view.printable_pressed.push((key, value));
                        send(Input::Unicode { value, down: true });
                    }
                    return true;
                } else if view.printable_pressed.iter().any(|(held, _)| *held == key)
                    || keyboard::scan_code(key)
                        .is_some_and(|(code, extended, _)| view.pressed.contains(&(code, extended)))
                {
                    keyboard::modifiers(view, *modifiers, send);
                    return true;
                }
            }
            Event::Text(_) if self.texts > 0 => {
                self.texts -= 1;
                return true;
            }
            _ => {}
        }
        false
    }
}
fn single_character(text: &str) -> Option<u16> {
    let mut chars = text.chars();
    let value = chars.next()? as u32;
    (chars.next().is_none() && (32..=65535).contains(&value) && value != 127)
        .then_some(value as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key: Key::Z,
            physical_key: Some(Key::Y),
            pressed,
            repeat,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn printable_keys_keep_os_text_until_release_and_ignore_client_typematic_repeats() {
        let mut view = DesktopView::default();
        let mut result = Vec::new();
        for events in [
            vec![key(true, false), Event::Text("Z".into())],
            vec![key(true, true), Event::Text("Z".into())],
            vec![key(false, false)],
        ] {
            let mut typing = Typing::new(&events, false);
            for event in events {
                assert!(typing.handle(&event, &mut view, &mut |input| result.push(input)));
            }
        }
        assert!(view.printable_pressed.is_empty());
        assert_eq!(result.len(), 2);
        assert!(matches!(
            result[0],
            Input::Unicode {
                value: 90,
                down: true
            }
        ));
        assert!(matches!(
            result[1],
            Input::Unicode {
                value: 90,
                down: false
            }
        ));
    }
    #[test]
    fn focus_loss_releases_printable_keys_and_ime_commits_use_shared_text_input() {
        let mut view = DesktopView::default();
        let events = [key(true, false), Event::Text("é".into())];
        let mut typing = Typing::new(&events, false);
        let mut result = Vec::new();
        for event in &events {
            typing.handle(event, &mut view, &mut |input| result.push(input));
        }
        keyboard::release(&mut view, &mut |input| result.push(input));
        assert!(matches!(
            result.last(),
            Some(Input::Unicode {
                value: 233,
                down: false
            })
        ));
        assert!(view.printable_pressed.is_empty());
        let commit = Event::Ime(egui::ImeEvent::Commit("中文".into()));
        assert!(!Typing::new(std::slice::from_ref(&commit), true).handle(
            &commit,
            &mut view,
            &mut |_| {}
        ));
    }
    #[test]
    fn changing_control_while_a_key_is_held_does_not_duplicate_or_orphan_it() {
        let mut view = DesktopView::default();
        view.pressed.push((0x15, false));
        view.modifiers.ctrl = true;
        let events = [key(true, true), Event::Text("z".into()), key(false, false)];
        let mut typing = Typing::new(&events, false);
        let mut result = Vec::new();
        for event in events {
            if !typing.handle(&event, &mut view, &mut |input| result.push(input)) {
                keyboard::handle(event, &mut view, &mut |input| result.push(input));
            }
        }
        assert!(view.pressed.is_empty() && view.printable_pressed.is_empty());
        assert!(
            !result
                .iter()
                .any(|input| matches!(input, Input::Unicode { down: true, .. }))
        );
        assert!(result.iter().any(|input| matches!(
            input,
            Input::ScanCode {
                code: 0x15,
                down: false,
                ..
            }
        )));
        let mut view = DesktopView::default();
        let events = [key(true, false), Event::Text("z".into())];
        let mut typing = Typing::new(&events, false);
        for event in &events {
            typing.handle(event, &mut view, &mut |_| {});
        }
        let repeat = Event::Key {
            key: Key::Z,
            physical_key: Some(Key::Y),
            pressed: true,
            repeat: true,
            modifiers: egui::Modifiers::CTRL,
        };
        assert!(Typing::default().handle(&repeat, &mut view, &mut |_| {}));
        assert!(view.pressed.is_empty());
        keyboard::release(&mut view, &mut |_| {});
        assert!(view.printable_pressed.is_empty());
        let mut view = DesktopView::default();
        let mut result = Vec::new();
        for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
            let event = Event::Key {
                key: Key::Backspace,
                physical_key: Some(Key::Backspace),
                pressed,
                repeat,
                modifiers: Default::default(),
            };
            if !Typing::default().handle(&event, &mut view, &mut |input| result.push(input)) {
                keyboard::handle(event, &mut view, &mut |input| result.push(input));
            }
        }
        assert_eq!(result.len(), 2);
        assert!(matches!(
            result[0],
            Input::ScanCode {
                code: 0x0e,
                down: true,
                ..
            }
        ));
        assert!(matches!(
            result[1],
            Input::ScanCode {
                code: 0x0e,
                down: false,
                ..
            }
        ));
    }
}
