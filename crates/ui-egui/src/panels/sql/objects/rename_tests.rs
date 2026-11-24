use super::*;
use egui::{Event, Key, Modifiers, PointerButton};
use tomato_core::{
    database_objects::{Catalog, Object},
    workbench::TableId,
};
struct Browser {
    ctx: egui::Context,
    state: State,
    icons: Icons,
    time: f64,
    actions: Vec<Action>,
}
impl Browser {
    fn new(mode: Mode) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut state = State::default();
        state.receive_shared(std::sync::Arc::new(Catalog {
            objects: [("table_name", false), ("view_name", true)]
                .into_iter()
                .map(|(name, view)| Object {
                    table: TableId {
                        schema: "owned".into(),
                        name: name.into(),
                    },
                    view,
                    comment: String::new(),
                    engine: String::new(),
                    auto_increment: String::new(),
                    update_time: String::new(),
                    data_length: String::new(),
                    rows: String::new(),
                    fields: Vec::new(),
                })
                .collect(),
            ..Default::default()
        }));
        state.mode = mode;
        Self {
            ctx,
            state,
            icons: Default::default(),
            time: 10.,
            actions: Vec::new(),
        }
    }
    fn frame(&mut self, mut events: Vec<Event>) -> egui::FullOutput {
        self.time += 0.02;
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                Event::PointerButton { modifiers, .. } | Event::Key { modifiers, .. } => {
                    Some(*modifiers)
                }
                _ => None,
            })
            .unwrap_or_default();
        events.insert(0, Event::ModifiersChanged(modifiers));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            },
            |ui| {
                if let Some(action) = render(
                    ui,
                    &mut self.state,
                    &mut self.icons,
                    Context {
                        id: "inline-object-rename",
                        busy: false,
                        error: None,
                    },
                ) {
                    self.actions.push(action);
                }
            },
        );
        output.textures_delta.clear();
        output
    }
    fn idle(&mut self, seconds: f64) {
        self.time += seconds;
        self.frame(vec![]);
    }
    fn point(&mut self, text: &str) -> Pos2 {
        self.frame(vec![])
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("actual {text} object control missing"))
    }
    fn click(&mut self, point: Pos2, button: PointerButton) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
    }
    fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
}
#[test]
fn table_and_view_names_use_delayed_inline_edit_without_opening_or_moving_rows() {
    for mode in [Mode::Icon, Mode::Detail] {
        for (name, index) in [("table_name", 0), ("view_name", 1)] {
            let mut b = Browser::new(mode);
            let point = b.point(name);
            b.click(point, PointerButton::Primary);
            assert!(b.state.rename.is_none());
            b.idle(0.5);
            b.click(point, PointerButton::Primary);
            assert!(b.state.rename_due.is_some());
            b.idle(0.31);
            assert!(
                b.state
                    .rename
                    .as_ref()
                    .is_some_and(|edit| edit.index == index)
            );
            assert!(
                b.actions.is_empty(),
                "slow second click must not open the object"
            );
            assert!(!egui::Popup::is_any_open(&b.ctx));
            assert!(b.state.notice.is_none() && b.state.confirmation.is_none());
            b.frame(vec![Event::Text("new_object_name".into())]);
            assert_eq!(b.state.rename.as_ref().unwrap().text, "new_object_name");
            b.key(Key::Enter);
            assert!(
                matches!(b.actions.last(),Some(Action::Rename(found,text)) if *found==index && text=="new_object_name")
            );
            assert_eq!(b.state.catalog.objects[index].view, index == 1);
        }
    }
}
#[test]
fn table_and_view_context_menu_and_f2_share_inline_edit_and_escape_releases_history() {
    for mode in [Mode::Icon, Mode::Detail] {
        for name in ["table_name", "view_name"] {
            let mut b = Browser::new(mode);
            let point = b.point(name);
            b.click(point, PointerButton::Primary);
            b.click(point, PointerButton::Secondary);
            let rename = b.point("重命名");
            b.click(rename, PointerButton::Primary);
            b.frame(vec![]);
            assert!(b.state.rename.is_some());
            assert!(!egui::Popup::is_any_open(&b.ctx));
            let editor = b.ctx.memory(|m| m.focused()).unwrap();
            b.key(Key::Escape);
            assert!(b.state.rename.is_none());
            assert!(egui::TextEdit::load_state(&b.ctx, editor).is_none());
            b.idle(0.5);
            b.click(point, PointerButton::Primary);
            b.key(Key::F2);
            b.frame(vec![]);
            assert!(b.state.rename.is_some());
            assert!(b.state.notice.is_none() && b.state.confirmation.is_none());
            b.key(Key::Escape);
            assert!(
                b.actions.is_empty(),
                "cancellation must not submit a database mutation"
            );
        }
    }
}

#[test]
fn dragging_a_detail_cell_selects_rows_and_context_opens_the_whole_selection() {
    let mut b = Browser::new(Mode::Detail);
    let start = b.point("table_name");
    let end = b.point("view_name");
    b.frame(vec![
        Event::PointerMoved(start),
        Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    b.frame(vec![Event::PointerMoved(end)]);
    b.frame(vec![Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    assert_eq!(b.state.selection_order, [0, 1]);
    assert!(b.state.rename_due.is_none() && b.actions.is_empty());
    b.click(start, PointerButton::Secondary);
    assert_eq!(b.state.selection_order, [0, 1]);
    let design = b
        .frame(vec![])
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text == "设计表" => {
                Some(t.pos + t.galley.size() / 2.)
            }
            _ => None,
        })
        .next_back()
        .unwrap();
    b.click(design, PointerButton::Primary);
    assert!(matches!(
        b.actions.last(),
        Some(Action::OpenSelected { structure: true })
    ));
}
