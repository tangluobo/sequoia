//! Native framebuffer scenarios. Synthetic egui input is limited to an owned empty profile.
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, RawInput};
pub(super) fn install(ctx: &Context) -> bool {
    let Ok(mode) = std::env::var("TOMATO_DESIGNER_INDEX") else {
        return false;
    };
    if !["base", "type", "unique", "method", "fields", "rename"].contains(&mode.as_str())
        || std::env::var("TOMATO_DESIGNER_PAGE").as_deref() != Ok("1")
        || !std::env::var_os("TOMATO_DATA_DIR")
            .map(std::path::PathBuf::from)
            .is_some_and(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("tomato-designer-fixture-"))
            })
    {
        return false;
    }
    let id = Id::new("designer-index-input-installed");
    if !ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        ctx.data_mut(|d| d.insert_temp(id, true));
        ctx.add_plugin(Input { mode, frame: 0 });
    }
    true
}
pub(super) fn record(ctx: &Context, tab: &crate::tabs::Tab) {
    if std::env::var_os("TOMATO_DESIGNER_INDEX").is_none() {
        return;
    }
    let crate::tabs::Panel::Sql { workbench, .. } = &tab.panel else {
        return;
    };
    let Some(draft) = &workbench.designer else {
        return;
    };
    let popup = ctx.data(|d| d.get_temp::<egui::Rect>(Id::new("designer-index-popup")));
    let field_popup =
        ctx.data(|d| d.get_temp::<egui::Rect>(Id::new("designer-index-fields-popup")));
    let fields = ctx
        .data(|d| d.get_temp::<Vec<(String, egui::Rect)>>(Id::new("designer-index-field-rows")))
        .unwrap_or_default();
    let fields:Vec<_>=fields.iter().map(|(name,r)|serde_json::json!({"name":name,"rect":[r.left(),r.top(),r.width(),r.height()]})).collect();
    let proof = serde_json::json!({"mode":std::env::var("TOMATO_DESIGNER_INDEX").unwrap_or_default(),"indexes":draft.request.indexes,"fieldsOpen":draft.indexes.fields.is_some(),"fieldRows":fields,"fieldPopup":field_popup.map(|r|[r.left(),r.top(),r.width(),r.height()]),"editing":draft.indexes.editor.is_some(),"dirty":draft.dirty,"popup":popup.map(|r|[r.left(),r.top(),r.width(),r.height()]),"osInputUsed":false,"savedHostsContacted":false});
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from) {
        let _ = std::fs::write(path.with_extension("index.json"), proof.to_string());
    }
}
struct Input {
    mode: String,
    frame: usize,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned designer index input"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        if input.viewport_id != egui::ViewportId::from_hash_of("table-designer-fixture") {
            return;
        }
        self.frame += 1;
        input.focused = true;
        let point = match self.mode.as_str() {
            "base" => None,
            "type" | "unique" => Some(egui::pos2(510., 140.)),
            "method" => Some(egui::pos2(630., 140.)),
            "fields" => Some(egui::pos2(300., 140.)),
            _ => Some(egui::pos2(80., 140.)),
        };
        if let Some(point) = point
            && [12, 13].contains(&self.frame)
        {
            input.events.push(Event::PointerMoved(point));
            input.events.push(Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: self.frame == 12,
                modifiers: Modifiers::NONE,
            });
        }
        if self.mode == "rename" {
            if self.frame == 20 {
                input.events.push(Event::Key {
                    key: Key::F2,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            if self.frame == 28 {
                input.events.push(Event::Text("ix_renamed".into()));
            }
            if self.frame == 30 {
                input.events.push(Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
        }
        if self.mode == "unique"
            && [24, 25].contains(&self.frame)
            && let Some(rect) =
                ctx.data(|d| d.get_temp::<egui::Rect>(Id::new("designer-index-popup")))
        {
            let point = rect.min + egui::vec2(40., 44.);
            input.events.push(Event::PointerMoved(point));
            input.events.push(Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: self.frame == 24,
                modifiers: Modifiers::NONE,
            });
        }
        ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
    }
}
