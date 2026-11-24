//! Open a real table through the normal controller, on an owned loopback fixture.
use super::*;
use tomato_core::workbench::{TableId, TableInfo};

impl TomatoApp {
    pub(super) fn preview_binary_table(&mut self) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Missing isolated profile"))?;
        let schema = std::env::var("TOMATO_BINARY_SCHEMA")?;
        let port: u16 = std::env::var("TOMATO_BINARY_PORT")?.parse()?;
        anyhow::ensure!(
            profile.is_absolute()
                && profile.file_name().is_some_and(|name| name
                    .to_string_lossy()
                    .starts_with("tomato-binary-fixture-"))
                && self.store.connections.is_empty()
                && port > 0
                && schema
                    .strip_prefix("tomato_binary_native_")
                    .is_some_and(|suffix| suffix.len() == 32
                        && suffix.bytes().all(|value| value.is_ascii_hexdigit())),
            "Owned, empty profile and loopback fixture required"
        );
        let mut connection = Connection::new("MYSQL");
        connection.name = "二进制字段验收".into();
        connection.set("host", "127.0.0.1");
        connection.set("port", u64::from(port));
        connection.set("username", "root");
        connection.set("database", schema.clone());
        self.quit_approved = true;
        self.open_table(
            connection,
            TableInfo {
                table: TableId {
                    schema,
                    name: "typed_values".into(),
                },
                view: false,
            },
            false,
        );
        Ok(())
    }

    pub(super) fn binary_table_preview(&self, ctx: &egui::Context) {
        if std::env::var("TOMATO_UI_PREVIEW").as_deref() != Ok("binary_table") {
            return;
        }
        let recorded = egui::Id::new("binary-table-proof-recorded");
        if ctx
            .data(|data| data.get_temp::<bool>(recorded))
            .unwrap_or(false)
        {
            return;
        }
        let Some(tab) = self.tabs.last() else { return };
        let Panel::Sql { workbench, .. } = &tab.panel else {
            return;
        };
        if tab.busy {
            return;
        }
        let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) else {
            return;
        };
        let page = workbench.page.as_ref();
        let proof = serde_json::json!({
            "ready":page.is_some() && tab.error.is_none(), "error":tab.error,
            "rows":page.map(|page| &page.rows), "total":page.map(|page| page.total),
            "columns":page.map(|page| page.definition.columns.iter().map(|column| serde_json::json!({"name":column.name,"binary":column.binary})).collect::<Vec<_>>()),
            "databaseRequested":true,"ownedLoopback":true,"savedHostsContacted":false,
            "configurationWritten":false,"osInputUsed":false,
        });
        if std::fs::write(path.with_extension("binary.json"), proof.to_string()).is_ok() {
            ctx.data_mut(|data| data.insert_temp(recorded, true));
        }
    }
}
