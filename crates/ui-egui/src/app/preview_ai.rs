//! Opt-in isolated AI dialogs; never performs an HTTP request or saves a setting.
use super::*;
impl TomatoApp {
    pub(super) fn preview_ai_sql(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("AI 夹具必须使用隔离目录"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-ai-sql-fixture-"))
                && self.store.connections.is_empty(),
            "AI 夹具必须使用独立空配置"
        );
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("ai_sql_settings") {
            self.open_ai_settings(None);
        } else {
            self.settings
                .extra
                .insert("aiModel".into(), "fixture-model".into());
            let mut tab = tabs::create_tab(Connection::new("SQLITE"), None)?;
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                workbench.schema = "main".into();
            }
            let id = tab.id.clone();
            self.active = Some(id.clone());
            self.tabs.push(tab);
            self.request_ai_sql(&id, false);
        }
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-ai-dialog-fixture"), true));
        Ok(())
    }
}
