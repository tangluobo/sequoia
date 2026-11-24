//! Right-click only inside this app's empty-profile object browser fixture.
use egui::{Context, Event, Id, PointerButton, RawInput};

pub(super) fn install(ctx: &Context, tab: &str) {
    let Ok(mode) = std::env::var("TOMATO_DESIGNER_MENU") else {
        return;
    };
    if !matches!(mode.as_str(), "item" | "background")
        || !matches!(
            std::env::var("TOMATO_DESIGNER_PAGE").as_deref(),
            Ok("7" | "8")
        )
        || !std::env::var_os("TOMATO_DATA_DIR")
            .map(std::path::PathBuf::from)
            .is_some_and(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("tomato-designer-fixture-")
                })
            })
    {
        return;
    }
    let installed = Id::new("objects-menu-native-input");
    if ctx
        .data(|data| data.get_temp::<bool>(installed))
        .unwrap_or(false)
    {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(installed, true));
    ctx.add_plugin(Input {
        mode,
        tab: tab.to_owned(),
        frame: 0,
    });
}

struct Input {
    mode: String,
    tab: String,
    frame: usize,
}

impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned object menu fixture"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::from_hash_of("table-designer-fixture") {
            return;
        }
        self.frame += 1;
        if matches!(self.frame, 12 | 13) {
            let pos = if self.mode == "item" {
                egui::pos2(40., 80.)
            } else {
                egui::pos2(400., 240.)
            };
            input.events.push(Event::PointerMoved(pos));
            input.events.push(Event::PointerButton {
                pos,
                button: PointerButton::Secondary,
                pressed: self.frame == 12,
                modifiers: Default::default(),
            });
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }

    fn output_hook(&mut self, ctx: &Context, output: &mut egui::FullOutput) {
        if self.frame <= 18 {
            return;
        }
        let target = if self.mode == "item" {
            // Find the actual first visible row rather than assuming catalog sorting.
            (0_usize..3)
                .map(|index| Id::new(("object-row", self.tab.as_str(), index)))
                .find(|id| {
                    ctx.read_response(id.with("popup").with("move"))
                        .is_some_and(|response| response.rect.width() > 1.)
                })
        } else {
            Some(Id::new(("objects-background", self.tab.as_str())))
        };
        let Some(rect) = target
            .and_then(|id| ctx.read_response(id.with("popup").with("move")))
            .map(|r| r.rect)
        else {
            return;
        };
        let positions: Vec<_> = output.shapes.iter().filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape && rect.contains(text.pos)
                && ["打开表", "设计表", "新建表", "新建查询", "复制表", "清空表", "截断表", "删除表", "刷新"].contains(&text.galley.job.text.as_str()) {
                Some(serde_json::json!({"label": text.galley.job.text, "x":text.pos.x, "y":text.pos.y,"height":text.galley.size().y}))
            } else { None }
        }).collect();
        if positions.len() == 9
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let _ = std::fs::write(path.with_extension("menu.json"), serde_json::json!({"mode":self.mode,"rect":[rect.left(),rect.top(),rect.width(),rect.height()],"positions":positions,"pointerEventsInjected":true,"osInputUsed":false,"networkRequested":false}).to_string());
        }
    }
}
