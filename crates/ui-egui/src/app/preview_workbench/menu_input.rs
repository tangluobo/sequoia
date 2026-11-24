//! Native fixture input goes through egui's public input hook, not OS/user-window automation.
use egui::{Context, Event, PointerButton, RawInput};
pub(super) fn install(ctx: &Context) {
    let Ok(mode) = std::env::var("TOMATO_TABLE_STATE") else {
        return;
    };
    if !matches!(
        mode.as_str(),
        "row-menu" | "multi-row-menu" | "cell-menu" | "filter" | "sort"
    ) {
        return;
    }
    if std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("table_data")
        || std::env::var_os("TOMATO_UI_CAPTURE").is_none()
    {
        return;
    }
    if !std::env::var_os("TOMATO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .is_some_and(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("tomato-sql-workbench-fixture-")
            })
        })
    {
        return;
    }
    let installed = egui::Id::new("table-menu-native-input");
    if ctx
        .data(|data| data.get_temp::<bool>(installed))
        .unwrap_or(false)
    {
        return;
    }
    ctx.data_mut(|data| {
        data.insert_temp(installed, true);
        data.insert_temp(egui::Id::new("table-clipboard-available"), false);
    });
    ctx.add_plugin(Input {
        mode,
        frame: 0,
        anchor: None,
    });
}
struct Input {
    mode: String,
    frame: usize,
    anchor: Option<egui::Pos2>,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned table menu fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::from_hash_of("sql-workbench-fixture") {
            return;
        }
        self.frame += 1;
        let mut sequence = vec![(
            egui::pos2(if self.mode == "cell-menu" { 110. } else { 8. }, 78.),
            PointerButton::Primary,
            egui::Modifiers::NONE,
        )];
        if self.mode == "multi-row-menu" {
            sequence.push((
                egui::pos2(8., 134.),
                PointerButton::Primary,
                egui::Modifiers::SHIFT,
            ));
        }
        sequence.push((
            egui::pos2(
                if self.mode == "cell-menu" { 110. } else { 8. },
                if self.mode == "multi-row-menu" {
                    106.
                } else {
                    78.
                },
            ),
            PointerButton::Secondary,
            egui::Modifiers::NONE,
        ));
        if matches!(self.mode.as_str(), "filter" | "sort") {
            sequence = self
                .anchor
                .into_iter()
                .map(|point| (point, PointerButton::Primary, egui::Modifiers::NONE))
                .collect();
        }
        if let Some(index) = self
            .frame
            .checked_sub(12)
            .filter(|index| *index < sequence.len() * 2)
        {
            let (point, button, modifiers) = sequence[index / 2];
            input.events.push(Event::ModifiersChanged(modifiers));
            input.events.push(Event::PointerMoved(point));
            input.events.push(Event::PointerButton {
                pos: point,
                button,
                pressed: index % 2 == 0,
                modifiers,
            });
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }
    fn output_hook(&mut self, _ctx: &Context, output: &mut egui::FullOutput) {
        let mut positions = Vec::new();
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                let label = &text.galley.job.text;
                if matches!(self.mode.as_str(), "filter" | "sort")
                    && self.anchor.is_none()
                    && label
                        == if self.mode == "filter" {
                            "筛选"
                        } else {
                            "排序"
                        }
                {
                    self.anchor = Some(text.pos + egui::vec2(3., 5.));
                }
                if [
                    "设置为空字符串",
                    "设置为 NULL",
                    "删除 记录",
                    "复制",
                    "复制为",
                    "粘贴",
                    "保存数据为...",
                    "排序",
                    "筛选",
                    "移除全部排序及筛选",
                    "显示",
                    "刷新",
                ]
                .contains(&label.as_str())
                {
                    positions.push(serde_json::json!({"label":label,"x":text.pos.x,"y":text.pos.y,"height":text.galley.size().y}));
                }
            }
        }
        if self.frame > 20
            && positions
                .iter()
                .any(|position| position["label"] == "删除 记录")
            && let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
        {
            let _=std::fs::write(path.with_extension("menu.json"),serde_json::json!({"mode":self.mode,"nativeInputFrames":self.frame,"pointerEventsInjected":true,"osInputUsed":false,"positions":positions}).to_string());
        }
    }
}
