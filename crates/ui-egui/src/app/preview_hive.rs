//! Opt-in release framebuffer, using the same owned HS2 peer as the control test.
use super::*;
impl TomatoApp {
    pub(super) fn preview_hive_designer(&mut self) -> anyhow::Result<()> {
        let profile = PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR")
                .ok_or_else(|| anyhow::anyhow!("Missing owned profile"))?,
        );
        anyhow::ensure!(
            profile.is_absolute()
                && profile
                    .file_name()
                    .is_some_and(|p| p.to_string_lossy().starts_with("tomato-hive-ui-fixture-"))
                && self.store.connections.len() == 1,
            "Requires isolated HS2 profile"
        );
        let c = self.store.connections[0].clone();
        anyhow::ensure!(
            c.kind() == "HIVE" && c.text("host") == "127.0.0.1",
            "Requires owned loopback Hive"
        );
        self.open_connection(c.clone());
        self.open_table(
            c,
            tomato_core::workbench::TableInfo {
                table: tomato_core::workbench::TableId {
                    schema: "default".into(),
                    name: "items".into(),
                },
                view: false,
            },
            true,
        );
        anyhow::ensure!(
            self.credential.is_none(),
            "NoSASL fixture unexpectedly requested a password"
        );
        Ok(())
    }
}
