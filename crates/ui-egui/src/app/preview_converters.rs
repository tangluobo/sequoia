//! Opt-in application captures can only convert files in a dedicated empty profile.
use super::*;
impl TomatoApp {
    pub(super) fn preview_converter(&mut self, kind: &str) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("转换验收需要隔离配置"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("tomato-converter-fixture-"))
                && self.store.connections.is_empty()
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "转换验收只能使用专有空配置"
        );
        let profile_display = profile.clone();
        let profile = profile.canonicalize()?;
        anyhow::ensure!(
            self.repository
                .location()
                .and_then(|p| p.parent())
                .and_then(|p| p.canonicalize().ok())
                .as_ref()
                == Some(&profile),
            "转换验收配置位置不匹配"
        );
        let source = std::env::var_os("TOMATO_UI_PREVIEW_PATH").map(PathBuf::from);
        let run = std::env::var("TOMATO_UI_CONVERTER_RUN").as_deref() == Ok("owned-files");
        let source = if let Some(source) = source {
            let canonical = source.canonicalize()?;
            anyhow::ensure!(
                canonical.starts_with(&profile),
                "转换验收源文件必须位于隔离配置目录内"
            );
            Some(source)
        } else {
            None
        };
        anyhow::ensure!(!run || source.is_some(), "转换验收缺少源文件");
        self.section = 1;
        self.open_tool(
            kind,
            if kind == "dataset_converter" {
                "数据集格式转换"
            } else {
                "图片格式转换"
            },
        );
        let Some(tab) = self.tabs.last_mut() else {
            return Ok(());
        };
        let Panel::Tool(tool) = &mut tab.panel else {
            return Ok(());
        };
        let id = tab.id.clone();
        if let Some(source) = source {
            tool.source = source.display().to_string();
            tool.destination = profile_display.join("converted").display().to_string();
        }
        let command = run
            .then(|| {
                tool.converter.get_or_insert_with(Default::default).start(
                    kind == "dataset_converter",
                    &tool.source,
                    &tool.destination,
                )
            })
            .flatten();
        if let Some(command) = command {
            self.context
                .data_mut(|d| d.insert_temp(egui::Id::new("owned-converter-installed"), true));
            self.submit(&id, command, Purpose::ToolConversion);
        }
        Ok(())
    }
    pub(super) fn preview_converter_ready(&self, ctx: &egui::Context) {
        if !ctx
            .data(|d| d.get_temp::<bool>(egui::Id::new("owned-converter-installed")))
            .unwrap_or(false)
            || ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new("owned-converter-ready")))
                .unwrap_or(false)
        {
            return;
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|t| Some(&t.id) == self.active.as_ref())
        else {
            return;
        };
        let Panel::Tool(tool) = &tab.panel else {
            return;
        };
        let Some(state) = &tool.converter else { return };
        if tab.busy || state.running() || state.status.is_empty() {
            return;
        }
        let proof = serde_json::json!({"status":state.status,"error":state.error,"busy":tab.busy,"inlineError":tab.error.is_some(),"physicalOsInputUsed":false});
        if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from)
            && let Ok(bytes) = serde_json::to_vec_pretty(&proof)
            && std::fs::write(path.with_extension("conversion.json"), bytes).is_ok()
        {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("owned-converter-ready"), true));
            ctx.request_repaint();
        }
    }
}
