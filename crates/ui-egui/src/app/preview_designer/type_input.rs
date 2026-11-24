//! Synthetic egui events only in the empty-profile designer capture fixture.
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, RawInput};

pub(super) fn install(ctx: &Context) -> bool {
    let Ok(mode) = std::env::var("TOMATO_DESIGNER_TYPE") else {
        return false;
    };
    if !matches!(
        mode.as_str(),
        "open" | "current" | "filter" | "commit" | "cancel" | "blur" | "text"
    ) || std::env::var("TOMATO_DESIGNER_PAGE").as_deref() != Ok("0")
        || !std::env::var_os("TOMATO_DATA_DIR")
            .map(std::path::PathBuf::from)
            .is_some_and(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("tomato-designer-fixture-")
                })
            })
    {
        return false;
    }
    let id = Id::new("designer-type-input-installed");
    if ctx.data(|data| data.get_temp::<bool>(id)).unwrap_or(false) {
        return true;
    }
    ctx.data_mut(|data| data.insert_temp(id, true));
    ctx.add_plugin(Input { mode, frame: 0 });
    true
}
pub(super) fn record(ctx: &Context, tab: &crate::tabs::Tab) {
    if std::env::var_os("TOMATO_DESIGNER_TYPE").is_none() {
        return;
    }
    let crate::tabs::Panel::Sql { workbench, .. } = &tab.panel else {
        return;
    };
    let Some(draft) = &workbench.designer else {
        return;
    };
    let picker = draft
        .edit
        .as_ref()
        .and_then(|edit| edit.type_picker.as_ref());
    let rect = picker
        .and_then(|picker| picker.popup)
        .and_then(|id| ctx.memory(|memory| memory.area_rect(id)));
    let positions = picker.and_then(|picker| picker.popup).map(|id| ctx.graphics(|graphics| {
        graphics.get(egui::LayerId::new(egui::Order::Foreground, id)).map(|list| list.all_entries().filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some(serde_json::json!({"text":text.galley.job.text,"x":text.pos.x,"y":text.pos.y,"width":text.galley.size().x,"height":text.galley.size().y}))
            } else { None }
        }).collect::<Vec<_>>()).unwrap_or_default()
    })).unwrap_or_default();
    let proof = serde_json::json!({"values":draft.request.columns.iter().map(|column| &column.data_type).collect::<Vec<_>>(),
        "dirty":draft.dirty,"revision":draft.revision,"editing":draft.edit.is_some(),
        "open":picker.is_some_and(|picker| picker.open),
        "focused":picker.and_then(|picker| picker.focused).and_then(|index| draft.types().get(index)),
        "visibleCount":picker.map(|picker| draft.types().iter().filter(|value| picker.matches(value)).count()),
        "popup":rect.map(|rect| [rect.left(), rect.top(), rect.width(), rect.height()]),
        "databaseRequested":false,"systemClipboardWritten":false,"osInputUsed":false,
        "fixtureFrame":ctx.data(|data| data.get_temp::<usize>(Id::new("designer-type-frame"))),
        "editText":draft.edit.as_ref().map(|edit| &edit.text),
        "mode":std::env::var("TOMATO_DESIGNER_TYPE").unwrap_or_default(),"textPositions":positions});
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from) {
        let _ = std::fs::write(path.with_extension("type.json"), proof.to_string());
    }
}
struct Input {
    mode: String,
    frame: usize,
}
fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned designer type fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        // Only this opt-in fixture owns these viewports. Preserve framebuffer events,
        // and keep incidental physical input from changing the synthetic scenario.
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        if input.viewport_id != egui::ViewportId::from_hash_of("table-designer-fixture") {
            return;
        }
        self.frame += 1;
        // This inactive, owned capture viewport receives synthetic egui keyboard input.
        // Keep its logical focus independent of the user's foreground application.
        input.focused = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("designer-type-frame"), self.frame));
        if self.mode == "text" {
            match self.frame {
                12 => input.events.push(key(Key::F2, Modifiers::NONE)),
                18 => input.events.push(Event::Text("id_edited".into())),
                _ => {}
            }
            ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
            return;
        }
        let pointer = match self.frame {
            12 | 13 => Some((
                egui::pos2(205., if self.mode == "current" { 133. } else { 103. }),
                self.frame == 12,
            )),
            22 | 23 if !matches!(self.mode.as_str(), "open" | "current") => {
                Some((egui::pos2(205., 103.), self.frame == 22))
            }
            36 | 37 if self.mode == "blur" => Some((egui::pos2(760., 380.), self.frame == 36)),
            _ => None,
        };
        if let Some((pos, pressed)) = pointer {
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        if !matches!(self.mode.as_str(), "open" | "current") {
            match self.frame {
                25 => input.events.push(key(
                    Key::A,
                    Modifiers {
                        command: true,
                        ..Modifiers::CTRL
                    },
                )),
                26 => input.events.push(Event::Text(
                    if self.mode == "blur" {
                        "CUSTOM_TYPE"
                    } else {
                        "chaR"
                    }
                    .into(),
                )),
                36 if self.mode == "commit" => {
                    input.events.push(key(Key::ArrowDown, Modifiers::NONE))
                }
                38 if self.mode == "commit" => input.events.push(key(Key::Enter, Modifiers::NONE)),
                38 if self.mode == "cancel" => input.events.push(key(Key::Escape, Modifiers::NONE)),
                _ => {}
            }
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }
}
