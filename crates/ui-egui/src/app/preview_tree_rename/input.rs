use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput};
pub(super) fn install(ctx: &Context, label: String) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("tree-rename-native-probe"), true));
    ctx.add_plugin(Input {
        label,
        target: None,
        step: 0,
        due: 0.,
        done: false,
    });
}
struct Input {
    label: String,
    target: Option<Pos2>,
    step: usize,
    due: f64,
    done: bool,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned sidebar inline rename fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input.focused = true;
        if self.done {
            return;
        }
        let now = input.time.unwrap_or_else(|| ctx.input(|i| i.time));
        input.events.push(Event::ModifiersChanged(Modifiers::NONE));
        if let Some(pos) = self.target
            && now >= self.due
            && self.step < 4
        {
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: self.step.is_multiple_of(2),
                modifiers: Modifiers::NONE,
            });
            self.step += 1;
            if self.step == 2 {
                self.due = now + 0.7;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        if self.target.is_none() {
            self.target = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == self.label => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            });
        }
        if self.done {
            return;
        }
        if let Some(mut trace) =
            ctx.data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("tree-rename-native-state")))
            && trace["focused"] == true
            && self.step == 4
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from)
        {
            trace["syntheticPointerEvents"] = serde_json::json!(4);
            trace["osInputUsed"] = serde_json::json!(false);
            trace["targetLabel"] = serde_json::json!(self.label);
            trace["target"] = serde_json::json!(self.target.map(|p| [p.x, p.y]));
            if std::fs::write(path.with_extension("rename.json"), trace.to_string()).is_ok() {
                self.done = true;
            }
        }
    }
}
use std::path::PathBuf;
