//! Input belongs exclusively to preview_image's validated empty loopback profile.
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput};
pub(super) fn install(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("file-rename-native-probe"), true));
    ctx.add_plugin(Input {
        target: None,
        step: 0,
        due: 0.,
        done: false,
    });
}
struct Input {
    target: Option<Pos2>,
    step: usize,
    due: f64,
    done: bool,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned inline rename fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        // The fixture does not take operating-system focus or send OS input.
        input.focused = true;
        if self.done {
            return;
        }
        let now = input.time.unwrap_or_else(|| ctx.input(|i| i.time));
        input.events.push(Event::ModifiersChanged(Modifiers::NONE));
        if let Some(point) = self.target
            && now >= self.due
        {
            let pressed = match self.step {
                0 | 2 => Some(true),
                1 | 3 => Some(false),
                _ => None,
            };
            if let Some(pressed) = pressed {
                input.events.push(Event::PointerMoved(point));
                input.events.push(Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
                self.step += 1;
                if self.step == 2 {
                    self.due = now + 0.4;
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        if self.target.is_none() {
            self.target = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "中文.png" => {
                        Some(text.pos + text.galley.size() / 2.)
                    }
                    _ => None,
                })
                .max_by(|a, b| a.x.total_cmp(&b.x));
        }
        if self.done {
            return;
        }
        if let Some(mut trace) = ctx.data(|data| {
            data.get_temp::<serde_json::Value>(egui::Id::new("file-rename-native-state"))
        }) && trace["focused"] == true
            && self.step == 4
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            trace["syntheticPointerEvents"] = serde_json::json!(4);
            trace["osInputUsed"] = serde_json::json!(false);
            trace["target"] = serde_json::json!(self.target.map(|point| [point.x, point.y]));
            if std::fs::write(path.with_extension("rename.json"), trace.to_string()).is_ok() {
                self.done = true;
            }
        }
    }
}
