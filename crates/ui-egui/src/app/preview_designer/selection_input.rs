//! Synthetic pointer frames in the existing owned native designer viewport.
use egui::{Context, Event, Id, Modifiers, PointerButton, RawInput};
pub(super) fn install(ctx: &Context) -> bool {
    let Ok(mode) = std::env::var("TOMATO_DESIGNER_SELECTION") else {
        return false;
    };
    if !matches!(
        mode.as_str(),
        "rows"
            | "shift"
            | "row-drag"
            | "reverse"
            | "cells"
            | "scroll"
            | "hold"
            | "batch"
            | "batch-reverse"
            | "batch-row"
    ) || std::env::var_os("TOMATO_DESIGNER_TYPE").is_some()
        || std::env::var("TOMATO_DESIGNER_PAGE").as_deref() != Ok("0")
        || !std::env::var_os("TOMATO_DATA_DIR")
            .map(std::path::PathBuf::from)
            .is_some_and(|path| {
                path.is_absolute()
                    && path.file_name().is_some_and(|name| {
                        name.to_string_lossy()
                            .starts_with("tomato-designer-fixture-")
                    })
            })
    {
        return false;
    }
    let installed = Id::new("designer-selection-input-installed");
    if !ctx
        .data(|data| data.get_temp::<bool>(installed))
        .unwrap_or(false)
    {
        ctx.data_mut(|data| data.insert_temp(installed, true));
        ctx.add_plugin(Input { mode, frame: 0 });
    }
    true
}
pub(super) fn record(ui: &egui::Ui, tab: &crate::tabs::Tab) {
    let ctx = ui.ctx();
    let Some(frame) = ctx.data(|data| data.get_temp::<usize>(Id::new("designer-selection-frame")))
    else {
        return;
    };
    // Input hooks run before egui switches to the incoming viewport. Read the
    // capture from the rendered child, rather than the previous parent window.
    if frame == 105 && std::env::var("TOMATO_DESIGNER_SELECTION").as_deref() == Ok("hold") {
        let captured = ctx.dragged_id().is_some();
        ctx.data_mut(|data| data.insert_temp(Id::new("designer-selection-held-capture"), captured));
    }
    let recorded = Id::new("designer-selection-proof-written");
    if frame < 150
        || ctx
            .data(|data| data.get_temp::<bool>(recorded))
            .unwrap_or(false)
    {
        return;
    }
    let crate::tabs::Panel::Sql { workbench, .. } = &tab.panel else {
        return;
    };
    let Some(draft) = &workbench.designer else {
        return;
    };
    let visible_fields = ctx.graphics(|graphics| {
        graphics
            .get(ui.layer_id())
            .map(|list| {
                list.all_entries()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            text.galley
                                .job
                                .text
                                .strip_prefix("column_")
                                .filter(|_| shape.clip_rect.contains(text.pos))
                                .and_then(|row| row.parse::<usize>().ok())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let proof = serde_json::json!({
        "mode":std::env::var("TOMATO_DESIGNER_SELECTION").unwrap_or_default(),"frame":frame,
        "selectedRows":draft.selected_rows(),"masks":(0..draft.request.columns.len()).map(|row|(0..8).fold(0u16,|mask,column|mask | if draft.selected(row,column){1<<column}else{0})).collect::<Vec<_>>(),
        "fieldCount":draft.request.columns.len(),"editing":draft.edit.is_some(),"dragging":draft.drag.is_some(),"dirty":draft.dirty,
        "focus":draft.focus.map(|cell|[cell.row,cell.column]),"popup":egui::Popup::is_any_open(ctx),
        "pointerCaptured":ctx.data(|data|data.get_temp::<bool>(Id::new("designer-selection-held-capture"))).unwrap_or(false),
        "visibleFieldRows":visible_fields,
        "values":draft.request.columns.iter().take(3).map(|column|(&column.name,&column.data_type)).collect::<Vec<_>>(),
        "databaseRequested":false,"systemClipboardWritten":false,"osInputUsed":false,
    });
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        && std::fs::write(path.with_extension("selection.json"), proof.to_string()).is_ok()
    {
        ctx.data_mut(|data| data.insert_temp(recorded, true));
    }
}
struct Input {
    mode: String,
    frame: usize,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned designer selection fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        if input.viewport_id != egui::ViewportId::from_hash_of("table-designer-fixture") {
            return;
        }
        self.frame += 1;
        input.focused = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("designer-selection-frame"), self.frame));
        let mut modifiers = Modifiers::NONE;
        let event = match self.mode.as_str() {
            "rows" | "shift" => match self.frame {
                12 | 13 => Some((egui::pos2(7., 103.), Some(self.frame == 12))),
                22 | 23 => {
                    modifiers = if self.mode == "rows" {
                        Modifiers::CTRL
                    } else {
                        Modifiers::SHIFT
                    };
                    Some((egui::pos2(7., 163.), Some(self.frame == 22)))
                }
                _ => None,
            },
            "row-drag" | "reverse" => match self.frame {
                12 => Some((
                    egui::pos2(7., if self.mode == "reverse" { 163. } else { 103. }),
                    Some(true),
                )),
                22 => Some((egui::pos2(400., 133.), None)),
                32 | 33 => Some((
                    egui::pos2(400., if self.mode == "reverse" { 103. } else { 163. }),
                    if self.frame == 33 { Some(false) } else { None },
                )),
                _ => None,
            },
            "cells" => match self.frame {
                12 => Some((egui::pos2(80., 103.), Some(true))),
                22 | 23 => Some((
                    egui::pos2(320., 163.),
                    if self.frame == 23 { Some(false) } else { None },
                )),
                _ => None,
            },
            "hold" => match self.frame {
                12 | 13 => Some((egui::pos2(80., 103.), Some(self.frame == 12))),
                22 => Some((egui::pos2(80., 103.), Some(true))),
                111 | 112 => Some((
                    egui::pos2(320., 163.),
                    if self.frame == 112 { Some(false) } else { None },
                )),
                _ => None,
            },
            "scroll" => match self.frame {
                12 => Some((egui::pos2(7., 103.), Some(true))),
                22..=119 => Some((egui::pos2(7., 393.), None)),
                120 => Some((egui::pos2(7., 393.), Some(false))),
                _ => None,
            },
            _ => None,
        };
        input.events.push(Event::ModifiersChanged(modifiers));
        if self.frame == 22 && matches!(self.mode.as_str(), "batch" | "batch-reverse" | "batch-row")
        {
            let (start, end) = match self.mode.as_str() {
                "batch-reverse" => (egui::pos2(320., 163.), egui::pos2(80., 103.)),
                "batch-row" => (egui::pos2(7., 103.), egui::pos2(400., 163.)),
                _ => (egui::pos2(80., 103.), egui::pos2(320., 163.)),
            };
            input.events.extend([
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
                Event::PointerMoved(egui::pos2(210., 133.)),
                Event::PointerMoved(end),
                Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            ]);
        }
        if let Some((pos, pressed)) = event {
            input.events.push(Event::PointerMoved(pos));
            if let Some(pressed) = pressed {
                input.events.push(Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers,
                });
            }
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }
}
