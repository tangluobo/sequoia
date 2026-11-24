//! Ctrl-drag inside the selection copies it to the drop caret, as in JavaFX.
use crate::tabs::completion::{CopyDrag, State};
use egui::{
    Color32, Event, Galley, Id, Key, LayerId, Order, PointerButton, Pos2, Rect, Stroke, StrokeKind,
    Ui, Vec2,
    text::{CCursor, CCursorRange},
    text_edit::{EventEdit, TextEditState},
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Gesture {
    pub suppress_selection: bool,
    pub drop: Option<Drop>,
}

pub(super) struct Drop {
    pub pos: Pos2,
    source: CCursorRange,
    insertion: usize,
    galley: Arc<Galley>,
}

impl Drop {
    pub fn insert(
        self,
        text: &str,
        range: CCursorRange,
    ) -> Result<Option<EventEdit>, &'static str> {
        // The ordered input stream may have edited the document earlier in this frame.
        if text != self.galley.job.text
            || range.as_sorted_char_range() != self.source.as_sorted_char_range()
        {
            return Ok(None);
        }
        let selected = self.source.slice_str(text);
        let bytes = text.len() + selected.len();
        if bytes > 8 * 1024 * 1024 {
            return Err("SQL 正文超过 8 MiB，未复制");
        }
        let at = super::completion::offset(text, self.insertion);
        let mut value = String::with_capacity(bytes);
        value.push_str(&text[..at]);
        value.push_str(selected);
        value.push_str(&text[at..]);
        let source = self.source.as_sorted_char_range();
        let count = usize::from(source.end) - usize::from(source.start);
        let range = CCursorRange::two(
            CCursor::new(self.insertion),
            CCursor::new(self.insertion + count),
        );
        Ok(Some(EventEdit::Replace(value, range)))
    }
}

enum PointerEvent {
    Button { pos: Pos2, down: bool, ctrl: bool },
    Move(Pos2),
    Cancel,
}

pub(super) fn track(ui: &Ui, id: Id, text: &str, state: &mut State) -> Gesture {
    let frame = ui.ctx().cumulative_frame_nr();
    if !ui.is_enabled()
        || !ui.input(|input| input.focused)
        || state
            .copy_drag
            .as_ref()
            .is_some_and(|drag| frame.saturating_sub(drag.frame) > 1)
    {
        state.copy_drag = None;
        return Gesture::default();
    }
    let mut gesture = Gesture {
        suppress_selection: state.copy_drag.is_some(),
        drop: None,
    };
    let Some(galley) = &state.galley else {
        return gesture;
    };
    let (events, ctrl, down) = ui.input(|input| {
        if state.copy_drag.is_none()
            && !input.events.iter().any(|event| {
                matches!(event, Event::PointerButton { button: PointerButton::Primary, pressed: true, modifiers, .. } if modifiers.ctrl)
            })
        {
            return (Vec::new(), false, false);
        }
        let events = input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::PointerButton {
                    pos,
                    pressed,
                    button: PointerButton::Primary,
                    modifiers,
                } => Some(PointerEvent::Button {
                    pos: *pos,
                    down: *pressed,
                    ctrl: modifiers.ctrl,
                }),
                Event::PointerMoved(pos) => Some(PointerEvent::Move(*pos)),
                Event::ModifiersChanged(modifiers) if !modifiers.ctrl => Some(PointerEvent::Cancel),
                Event::PointerGone
                | Event::WindowFocused(false)
                | Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => Some(PointerEvent::Cancel),
                _ => None,
            })
            .collect::<Vec<_>>();
        (events, input.modifiers.ctrl, input.pointer.primary_down())
    });
    let hit = |pos: Pos2| galley.cursor_from_pos(pos - state.galley_pos);
    for event in events {
        match event {
            PointerEvent::Button {
                pos,
                down: true,
                ctrl,
            } => {
                state.copy_drag = None;
                gesture.drop = None;
                let source = ctrl
                    .then(|| TextEditState::load(ui.ctx(), id))
                    .flatten()
                    .and_then(|state| state.cursor.char_range());
                if let Some(source) = source
                    && !source.is_empty()
                    && state.text_rect.is_some_and(|rect| rect.contains(pos))
                    && ui.clip_rect().contains(pos)
                    && ui.ctx().read_response(id).is_some()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id())
                    && galley.job.text == text
                {
                    let range = source.as_sorted_char_range();
                    let caret = hit(pos);
                    if range.start <= caret.index && caret.index <= range.end {
                        state.copy_drag = Some(CopyDrag {
                            source,
                            origin: pos,
                            frame,
                            preview: None,
                            cancelled: false,
                        });
                        gesture.suppress_selection = true;
                        ui.memory_mut(|memory| memory.request_focus(id));
                    }
                }
            }
            PointerEvent::Move(pos) => {
                if let Some(drag) = &mut state.copy_drag
                    && !drag.cancelled
                    && drag.preview.is_none()
                    && drag.origin.distance(pos) >= 3.
                {
                    drag.preview = Some(preview_lines(drag.source.slice_str(text)));
                }
            }
            PointerEvent::Button {
                pos,
                down: false,
                ctrl,
            } => {
                if let Some(drag) = state.copy_drag.take()
                    && ctrl
                    && !drag.cancelled
                    && drag.preview.is_some()
                {
                    gesture.drop = Some(Drop {
                        pos,
                        source: drag.source,
                        insertion: hit(pos).index.into(),
                        galley: galley.clone(),
                    });
                }
            }
            PointerEvent::Cancel => {
                gesture.drop = None;
                if let Some(drag) = &mut state.copy_drag {
                    drag.cancelled = true;
                    drag.preview = None;
                }
            }
        }
    }
    if let Some(drag) = &mut state.copy_drag {
        drag.frame = frame;
        if !ctrl {
            drag.cancelled = true;
            drag.preview = None;
        }
        if !down {
            state.copy_drag = None;
        }
    }
    gesture
}

fn preview_lines(text: &str) -> Vec<String> {
    // The preview borrows the editor text and keeps at most eight truncated lines.
    let mut lines = text
        .split('\n')
        .flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'));
    let mut visible = Vec::new();
    for line in lines.by_ref().take(8) {
        let mut chars = line.chars().flat_map(|ch| {
            let mut expanded = [' '; 4];
            let len = if ch == '\t' {
                4
            } else {
                expanded[0] = ch;
                1
            };
            expanded.into_iter().take(len)
        });
        let mut label = chars.by_ref().take(96).collect::<String>();
        if chars.next().is_some() {
            label.pop();
            label.push('…');
        }
        visible.push(label);
    }
    if lines.next().is_some() {
        visible.push("…".into());
    }
    visible
}

pub(super) fn paint(ui: &Ui, id: Id, state: &State) {
    let Some(lines) = state
        .copy_drag
        .as_ref()
        .and_then(|drag| drag.preview.as_ref())
    else {
        return;
    };
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    let painter = ui
        .ctx()
        .layer_painter(LayerId::new(Order::Tooltip, id.with("copy-drag")));
    let text_color = Color32::from_rgba_unmultiplied(31, 41, 55, 169);
    let galleys: Vec<_> = lines
        .iter()
        .map(|line| painter.layout_no_wrap(line.clone(), egui::FontId::monospace(12.), text_color))
        .collect();
    let width = galleys.iter().map(|line| line.size().x).fold(1., f32::max);
    let rect = Rect::from_min_size(
        pointer + Vec2::new(14., 18.),
        Vec2::new(width + 14., lines.len() as f32 * 17. + 10.),
    );
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(46),
        }
        .as_shape(rect, 4),
    );
    painter.rect(
        rect,
        4.,
        Color32::from_rgba_unmultiplied(94, 153, 219, 44),
        Stroke::new(1., Color32::from_rgba_unmultiplied(74, 144, 226, 143)),
        StrokeKind::Inside,
    );
    for (index, galley) in galleys.into_iter().enumerate() {
        painter.galley(
            rect.min + Vec2::new(7., 5. + index as f32 * 17.),
            galley,
            text_color,
        );
    }
}
