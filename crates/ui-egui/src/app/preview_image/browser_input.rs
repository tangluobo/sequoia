//! In-process egui input, enabled only after preview_image's disposable-profile checks.
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput};
pub(super) fn install(ctx: &Context, markdown: bool) {
    ctx.add_plugin(Input {
        markdown,
        target: None,
        step: 0,
        done: false,
    });
}
struct Input {
    markdown: bool,
    target: Option<Pos2>,
    step: usize,
    done: bool,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned file-browser controls"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input.focused = true;
        let count = if self.markdown { 4 } else { 2 };
        if !self.done
            && self.step < count
            && let Some(pos) = self.target
        {
            input.events.push(Event::ModifiersChanged(Modifiers::NONE));
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                button: if self.markdown {
                    PointerButton::Primary
                } else {
                    PointerButton::Secondary
                },
                pressed: self.step.is_multiple_of(2),
                modifiers: Modifiers::NONE,
            });
            self.step += 1;
        }
        if !self.done {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        let target = if self.markdown {
            "readme.md"
        } else {
            "中文.png"
        };
        if self.target.is_none() {
            self.target = texts
                .iter()
                .filter(|t| t.galley.job.text == target)
                .map(|t| t.pos + t.galley.size() / 2.)
                .max_by(|a, b| a.x.total_cmp(&b.x));
        }
        if self.done {
            return;
        }
        let ready = if self.markdown {
            texts
                .iter()
                .any(|t| t.galley.job.text.contains("Markdown 实际文件"))
        } else {
            texts.iter().any(|t| t.galley.job.text == "下载...")
        };
        if ready
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let labels: Vec<_> = texts.iter().map(|t| t.galley.job.text.as_str()).collect();
            let rows: Vec<_> = texts
                .iter()
                .filter(|t| {
                    [
                        "打开",
                        "预览图片",
                        "下载...",
                        "复制访问地址",
                        "重命名",
                        "视图",
                        "刷新",
                    ]
                    .contains(&t.galley.job.text.as_str())
                })
                .map(|t| serde_json::json!({"label":t.galley.job.text,"x":t.pos.x,"y":t.pos.y}))
                .collect();
            let trace = serde_json::json!({"markdown":self.markdown,"ready":true,"popup":egui::Popup::is_any_open(ctx),"syntheticPointerEvents":self.step,"osInputUsed":false,"labels":labels,"rows":rows});
            if std::fs::write(path.with_extension("browser.json"), trace.to_string()).is_ok() {
                self.done = true;
            }
        }
    }
}
