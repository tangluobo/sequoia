//! Owned opt-in framebuffer fixture; rejects installed profiles and SDK configurations.
use super::*;
impl State {
    pub(crate) fn preview_prepare(&mut self, mode: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.config.runtimes.is_empty() && self.config.build_tools.is_empty(),
            "开发目录验收夹具不可配置运行环境"
        );
        if !self.loaded || self.config.projects.is_empty() || mode == "empty" {
            return Ok(());
        }
        let root = std::fs::canonicalize(self.path.parent().unwrap())?;
        let project = std::fs::canonicalize(&self.config.projects[0].path)?;
        anyhow::ensure!(project.starts_with(&root), "开发项目必须属于隔离夹具目录");
        if self.selected.is_none() {
            self.navigate_now(Navigation::Project(0));
        }
        self.tab = match mode {
            "version" => 1,
            "dependencies" => 2,
            "logs" => 3,
            _ => 0,
        };
        let marker = egui::Id::new("development-fixture-opened");
        if mode == "editor"
            && self.overview.is_some()
            && !self
                .context
                .data(|d| d.get_temp::<bool>(marker))
                .unwrap_or(false)
        {
            let path = project.join("src/main/java/demo/App.java");
            self.navigate_now(Navigation::File(path));
            self.context.data_mut(|d| d.insert_temp(marker, true));
        }
        Ok(())
    }
}
