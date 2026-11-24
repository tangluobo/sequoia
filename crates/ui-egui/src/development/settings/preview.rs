//! Preview uses the real dialog bodies; returned actions are deliberately not dispatched.
use super::*;
use tomato_core::development::runtime::EntryRole;
impl State {
    pub(crate) fn settings_preview_size(mode: &str) -> Option<egui::Vec2> {
        match mode {
            "settings-add-directory" => Some(vec2(460., 220.)),
            "settings-project"
            | "settings-sdks"
            | "settings-sdks-selected"
            | "settings-builds"
            | "settings-builds-selected" => Some(vec2(660., 503.)),
            "settings-sdk-add" => Some(vec2(508., 203.)),
            "settings-tool-add"
            | "settings-tool-edit"
            | "settings-tool-gradle"
            | "settings-tool-type-change" => Some(vec2(573., 238.)),
            _ => None,
        }
    }
    pub(crate) fn preview_settings(&mut self, ui: &mut Ui, mode: &str) -> anyhow::Result<()> {
        if mode == "settings-add-directory" {
            add_directory::preview(ui, self);
            return Ok(());
        }
        if !self.loaded || self.config.projects.is_empty() {
            return Ok(());
        }
        if self.settings.is_none() {
            let fixture = self
                .config
                .extra
                .get("developmentSettingsFixture")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("缺少隔离设置夹具"))?;
            let fixture: Config = serde_json::from_value(fixture)?;
            let mut config = self.config.clone();
            config.runtimes = fixture.runtimes;
            config.build_tools = fixture.build_tools;
            let mut draft = Draft::new(0, &config);
            if mode.contains("sdks") {
                draft.tab = 1;
            }
            if mode.contains("builds") {
                draft.tab = 2;
            }
            if mode == "settings-sdks-selected" {
                draft.selected_runtime = Some(0);
            }
            if mode == "settings-builds-selected" {
                draft.selected_tool = Some(0);
            }
            match mode {
                "settings-sdk-add" => draft.begin(EntryRole::Runtime, None),
                "settings-tool-add" => draft.begin(EntryRole::BuildTool, None),
                "settings-tool-edit" | "settings-tool-type-change" => {
                    draft.begin(EntryRole::BuildTool, Some(0))
                }
                "settings-tool-gradle" => draft.begin(EntryRole::BuildTool, Some(1)),
                _ => {}
            }
            if mode == "settings-tool-type-change" {
                draft.kind = "GRADLE".into();
                draft.change_kind();
            }
            self.settings = Some(draft);
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0, egui::Color32::from_gray(244));
        let draft = self.settings.as_mut().unwrap();
        draft.bind(ui.ctx());
        if draft.edit.is_some() {
            entry::contents(ui, draft, false);
        } else {
            project::contents(ui, draft, false);
        }
        Ok(())
    }
}
