//! Isolated loopback fixture using the normal file actor, listing and image reader.
use super::*;
mod browser_input;
mod rename_input;
impl TomatoApp {
    pub(super) fn preview_image(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("image fixture profile missing"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-image-fixture-")),
            "image fixture profile name mismatch"
        );
        anyhow::ensure!(
            self.store.connections.len() == 1,
            "image fixture requires one isolated connection"
        );
        let connection = self.store.connections[0].clone();
        anyhow::ensure!(
            connection.kind() == "S3"
                && connection.name == "图片预览验收"
                && connection.text("bucket") == "tomato-image-fixture"
                && connection
                    .text("endpoint")
                    .strip_prefix("http://127.0.0.1:")
                    .and_then(|port| port.parse::<u16>().ok())
                    .is_some_and(|port| port != 0),
            "image fixture must use isolated loopback S3"
        );
        let browser_mode = std::env::var("TOMATO_IMAGE_BROWSER_MODE").ok();
        if let Some(mode) = &browser_mode {
            anyhow::ensure!(
                matches!(mode.as_str(), "ICON" | "LIST" | "COLUMN"),
                "invalid browser fixture mode"
            );
        }
        self.open_connection(connection);
        if let Some(tab) = self.tabs.last_mut()
            && let Panel::Terminal { view, .. } = &mut tab.panel
            && let Some(workspace) = &mut view.ssh
        {
            workspace.file_view = browser_mode.clone().unwrap_or_else(|| "ICON".into());
        }
        self.section = 0;
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-image-fixture"), true));
        if browser_mode.is_some() {
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("tomato-image-fixture-opened"), true)
            });
            if std::env::var("TOMATO_IMAGE_BROWSER_RENAME").as_deref() == Ok("1") {
                rename_input::install(ctx);
            }
            if std::env::var("TOMATO_IMAGE_BROWSER_MENU").as_deref() == Ok("1")
                || std::env::var("TOMATO_IMAGE_BROWSER_MARKDOWN").as_deref() == Ok("1")
            {
                browser_input::install(
                    ctx,
                    std::env::var("TOMATO_IMAGE_BROWSER_MARKDOWN").as_deref() == Ok("1"),
                );
            }
        }
        Ok(())
    }
}
