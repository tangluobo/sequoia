//! Inert application headers: no protocol start, database requests or shared config writes.
use super::*;
impl TomatoApp {
    pub(super) fn preview_tabs(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("标签夹具需要隔离目录"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-tabs-fixture-"))
                && self.store.connections.is_empty(),
            "标签夹具需要独立空配置"
        );
        let labels = [
            "生产服务器 SSH",
            "sample_db.items",
            "对象",
            "很长的数据库查询标题 SELECT FROM WHERE",
        ];
        for (index, label) in labels.into_iter().enumerate() {
            let mut tab = tabs::create_tab(
                Connection::new(if index == 1 || index == 2 {
                    "MYSQL"
                } else {
                    "SPICE"
                }),
                None,
            )?;
            tab.title = label.into();
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                if index == 1 {
                    workbench.table = Some(tomato_core::workbench::TableId {
                        schema: "sample_db".into(),
                        name: "items".into(),
                    });
                }
                if index == 2 {
                    workbench.browser = Some(Box::default());
                }
            }
            self.tabs.push(tab);
        }
        self.active = Some(
            self.tabs[if std::env::var("TOMATO_TAB_STATE").as_deref() == Ok("selected") {
                1
            } else {
                0
            }]
            .id
            .clone(),
        );
        if matches!(
            std::env::var("TOMATO_TAB_STATE").as_deref(),
            Ok("overflow" | "popup")
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(800., 600.)));
        }
        Ok(())
    }
}
