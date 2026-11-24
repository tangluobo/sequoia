//! Native capture fixture. Only an explicitly named, empty disposable profile can enable input.
use super::*;
mod input;
impl TomatoApp {
    pub(super) fn preview_tree_rename(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("missing owned tree profile"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|p| p
                .to_string_lossy()
                .starts_with("tomato-tree-rename-fixture-")),
            "unexpected tree profile"
        );
        anyhow::ensure!(
            self.store.connections.is_empty() && !profile.join("connections.json").exists(),
            "tree preview requires an empty profile"
        );
        let target = std::env::var("TOMATO_TREE_RENAME_TARGET")?;
        anyhow::ensure!(
            matches!(
                target.as_str(),
                "文件.md" | "文件夹" | "测试表" | "测试视图"
            ),
            "unexpected target"
        );
        let root = profile.join("files");
        anyhow::ensure!(
            root.join("文件.md").is_file() && root.join("文件夹").is_dir(),
            "missing tree files"
        );
        let mut local = Connection::new("LOCAL_DIRECTORY");
        local.name = "本地文件".into();
        local.set("localDirectoryPath", root.to_string_lossy().as_ref());
        let mut sql = Connection::new("MYSQL");
        sql.name = "数据库".into();
        sql.set("host", "127.0.0.1");
        sql.set("port", 0u64);
        sql.set("database", "owned");
        let mut host = sql_tree::Host::new(sql.clone());
        host.schemas = std::sync::Arc::new(vec!["owned".into()]);
        host.tables.insert(
            "owned".into(),
            std::sync::Arc::new(
                [("测试表", false), ("测试视图", true)]
                    .map(|(name, view)| tomato_core::workbench::TableInfo {
                        table: tomato_core::workbench::TableId {
                            schema: "owned".into(),
                            name: name.into(),
                        },
                        view,
                    })
                    .into(),
            ),
        );
        self.store.connections = vec![local.clone(), sql.clone()];
        self.sql_hosts.insert(sql.id.clone(), host);
        self.open_connection(local);
        self.collapsed.clear();
        self.section = 0;
        input::install(ctx, target);
        Ok(())
    }
}
