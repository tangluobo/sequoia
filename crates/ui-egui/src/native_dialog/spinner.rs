//! JavaFX Spinner hold timing. One mouse button can own one repeater at a time.
use egui::{Id, Response, Ui};
#[derive(Clone, Copy, Default)]
struct Held {
    owner: Option<Id>,
    next: f64,
}
pub(crate) fn repeat(ui: &Ui, response: &Response, id: Id) -> bool {
    let (pressed, down, now) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.time,
        )
    });
    let key = Id::new("native-spinner-repeater");
    let mut held = ui
        .ctx()
        .data(|data| data.get_temp::<Held>(key))
        .unwrap_or_default();
    let mut step = false;
    if pressed && response.is_pointer_button_down_on() {
        held = Held {
            owner: Some(id),
            next: now + 0.3,
        };
        step = true;
    } else if !down {
        held = Held::default();
    } else if held.owner == Some(id) && now >= held.next {
        step = true;
        held.next = now + 0.06;
    }
    if held.owner == Some(id) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                (held.next - now).max(0.001),
            ));
    }
    ui.ctx().data_mut(|data| data.insert_temp(key, held));
    step
}
