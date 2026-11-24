//! Shared ordered keyboard/IME handling for both database grids.
use crate::tabs::batch_edit::Edit;
use egui::{Event, Id, ImeEvent, Key, Ui};
pub(super) trait Target {
    fn active(&self) -> bool;
    fn composing(&self) -> bool;
    fn start(&mut self) -> anyhow::Result<bool>;
    fn edit(&mut self, edit: Edit<'_>) -> anyhow::Result<()>;
    fn finish(&mut self) -> anyhow::Result<()>;
    fn cancel(&mut self);
    fn error(&mut self, error: anyhow::Error);
}
/// Returns false when invalid input must be corrected before selecting another cell.
pub(super) fn handle(ui: &mut Ui, id: Id, enabled: bool, target: &mut impl Target) -> bool {
    let focused = ui.memory(|memory| memory.has_focus(id));
    if target.active()
        && ui.input(|input| input.key_pressed(Key::Escape))
        && (focused || ui.memory(|memory| memory.had_focus_last_frame(id)))
    {
        target.cancel();
        ui.input_mut(|input| {
            input.consume_key(input.modifiers, Key::Escape);
        });
        ui.memory_mut(|memory| memory.request_focus(id));
        return true;
    }
    if target.active()
        && (!focused || ui.input(|input| input.pointer.any_pressed()))
        && let Err(error) = target.finish()
    {
        target.error(error);
        ui.memory_mut(|memory| memory.request_focus(id));
        return false;
    }
    if !focused || !enabled {
        return true;
    }
    let events = ui.input(|input| {
        input
            .events
            .iter()
            .map(|event| match event {
                // Only the presence of paste matters here; leave its payload to the grid.
                Event::Paste(_) => Event::Paste(String::new()),
                event => event.clone(),
            })
            .collect::<Vec<_>>()
    });
    let mut modifiers = ui.input(|input| input.modifiers);
    let mut consumed = Vec::new();
    let mut valid = true;
    for (index, event) in events.iter().enumerate() {
        let edit = match event {
            Event::ModifiersChanged(value) => {
                modifiers = *value;
                None
            }
            Event::Text(text)
                if !modifiers.command
                    && !modifiers.alt
                    && !text.is_empty()
                    && text.chars().all(|ch| !ch.is_control()) =>
            {
                Some(Edit::Text(text))
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) if target.active() || !text.is_empty() => {
                Some(Edit::Preedit(text))
            }
            Event::Ime(ImeEvent::Commit(text)) if target.active() || !text.is_empty() => {
                Some(Edit::Commit(text))
            }
            Event::Key {
                key: Key::Backspace,
                pressed: true,
                ..
            } if target.active() && !target.composing() => Some(Edit::Backspace),
            _ => None,
        };
        if let Some(edit) = edit {
            match target
                .active()
                .then_some(true)
                .map_or_else(|| target.start(), Ok)
            {
                Ok(true) => {
                    if let Err(error) = target.edit(edit) {
                        target.error(error);
                        valid = false;
                    }
                    consumed.push(index);
                }
                Ok(false) => {}
                Err(error) => {
                    target.error(error);
                    valid = false;
                    consumed.push(index);
                }
            }
        } else if target.active() && target.composing() {
            if matches!(
                event,
                Event::Key {
                    key: Key::Enter
                        | Key::Tab
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::ArrowLeft
                        | Key::ArrowRight
                        | Key::Backspace,
                    pressed: true,
                    ..
                }
            ) {
                consumed.push(index);
            }
        } else if target.active() {
            let finish = matches!(event, Event::Paste(_))
                || matches!(
                    event,
                    Event::Key {
                        key: Key::Enter
                            | Key::Tab
                            | Key::ArrowUp
                            | Key::ArrowDown
                            | Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Home
                            | Key::End
                            | Key::PageUp
                            | Key::PageDown
                            | Key::F2,
                        pressed: true,
                        ..
                    }
                )
                || matches!(event,Event::Key{key:Key::A,pressed:true,modifiers,..} if modifiers.command);
            if finish {
                let enter = matches!(
                    event,
                    Event::Key {
                        key: Key::Enter,
                        ..
                    }
                );
                if let Err(error) = target.finish() {
                    target.error(error);
                    valid = false;
                    consumed.push(index);
                } else if enter {
                    consumed.push(index);
                }
            }
        }
    }
    if !consumed.is_empty() {
        let mut index = 0;
        ui.input_mut(|input| {
            input.events.retain(|_| {
                let keep = !consumed.contains(&index);
                index += 1;
                keep
            })
        });
        ui.ctx().request_repaint();
    }
    valid
}
pub(super) fn ime(ui: &Ui, rect: egui::Rect) {
    ui.ctx().output_mut(|output| {
        output.ime = Some(egui::output::IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect,
            cursor_rect: rect,
            should_interrupt_composition: false,
        })
    });
}
