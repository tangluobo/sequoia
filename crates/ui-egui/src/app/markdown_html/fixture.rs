//! Exercise the actual toolbar and native picker with an owned empty profile.
use super::*;

fn key() -> egui::Id {
    egui::Id::new("owned-markdown-html-fixture")
}
const READY: &str = "owned-markdown-html-ready";

impl TomatoApp {
    pub(in crate::app) fn preview_markdown_html(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("HTML 导出验收需要隔离配置目录"))?;
        anyhow::ensure!(
            self.store.connections.is_empty()
                && profile.file_name().is_some_and(|name| name
                    .to_string_lossy()
                    .starts_with("tomato-markdown-html-fixture-")),
            "HTML 导出验收只能使用专有空配置"
        );
        let profile = profile.canonicalize()?;
        let input = profile.join("中文 & 报告.md").canonicalize()?;
        anyhow::ensure!(
            input.starts_with(&profile)
                && self
                    .repository
                    .location()
                    .and_then(|path| path.parent())
                    .and_then(|path| path.canonicalize().ok())
                    .as_ref()
                    == Some(&profile),
            "HTML 导出验收文件必须在隔离目录内"
        );
        self.context.data_mut(|data| data.insert_temp(key(), true));
        self.open_editor(input);
        self.context.add_plugin(ClickHtml::default());
        Ok(())
    }
}

fn active(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(key()).unwrap_or(false))
}

pub(super) fn complete(ctx: &egui::Context, success: bool, cancelled: bool, message: &str) {
    let ready = egui::Id::new(READY);
    if active(ctx)
        && !ctx.data(|data| data.get_temp::<bool>(ready).unwrap_or(false))
        && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from)
    {
        let outcome =
            serde_json::json!({"success":success,"cancelled":cancelled,"message":message});
        if std::fs::write(path.with_extension("html.json"), outcome.to_string()).is_ok() {
            ctx.data_mut(|data| data.insert_temp(ready, true));
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
}

pub(super) fn select_viewport(ctx: &egui::Context, id: egui::ViewportId) {
    if active(ctx) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
    }
}

pub(super) fn capture(ui: &egui::Ui, id: egui::ViewportId) {
    if active(ui.ctx())
        && let Some(image) = ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot {
                    viewport_id, image, ..
                } if *viewport_id == id => Some(image.clone()),
                _ => None,
            })
        })
    {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("tomato-capture-image"), image);
        });
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
}

#[derive(Default)]
struct ClickHtml {
    position: Option<egui::Pos2>,
    phase: u8,
}

impl egui::plugin::Plugin for ClickHtml {
    fn debug_name(&self) -> &'static str {
        "owned Markdown HTML toolbar"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        if self.phase == 0 && ctx.viewport_id() == egui::ViewportId::ROOT {
            // The code-block tool has the first copy of this glyph. The second
            // appears only after the source document has loaded.
            self.position = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "\u{f1c9}" => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .nth(1);
        }
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT || self.phase >= 2 {
            return;
        }
        if let Some(pos) = self.position {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                pressed: self.phase == 0,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            });
            self.phase += 1;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(20));
    }
}
