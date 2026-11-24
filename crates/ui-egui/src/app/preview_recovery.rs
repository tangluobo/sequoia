//! Opt-in actual database/SSH capture, confined to the owned loopback fixture.
use super::*;
use egui::{Context, Id};
impl TomatoApp {
    pub(super) fn preview_database_recovery(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("恢复验收需要隔离配置"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-recovery-fixture-"))
                && self.store.connections.len() == 1
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "恢复验收仅允许独立夹具配置"
        );
        let connection = self.store.connections[0].clone();
        anyhow::ensure!(
            connection.kind() == "POSTGRESQL"
                && connection.text("host") == "127.0.0.1"
                && connection.text("database") == "tomato_fixture"
                && connection.text("username") == "tomato_fixture"
                && connection.flag("useSshTunnel")
                && connection.text("sshTunnelHost") == "127.0.0.1"
                && connection.text("sshTunnelUsername") == "fixture",
            "恢复验收仅允许自有数据库和 SSH 服务"
        );
        let mut tab = tabs::create_tab(connection.clone(), None)?;
        let source = "SELECT '保留中文结果' AS value;\nSELECT pg_terminate_backend(pg_backend_pid());\nSELECT '后续语句不应执行';";
        if let Panel::Sql { sql, workbench, .. } = &mut tab.panel {
            *sql = source.into();
            workbench.schema = "public".into();
        }
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.active = Some(id.clone());
        self.section = 0;
        self.quit_approved = true;
        self.submit(
            &id,
            Command::SqlScript {
                connection,
                sql: source.into(),
                mode: tomato_services::database::script::Mode::Run,
            },
            Purpose::Normal,
        );
        ctx.data_mut(|data| data.insert_temp(Id::new("owned-recovery-installed"), true));
        Ok(())
    }
    pub(super) fn database_recovery_preview(&mut self, ctx: &Context) {
        if !ctx
            .data(|data| data.get_temp::<bool>(Id::new("owned-recovery-installed")))
            .unwrap_or(false)
            || ctx
                .data(|data| data.get_temp::<bool>(Id::new("owned-recovery-ready")))
                .unwrap_or(false)
        {
            return;
        }
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return;
        };
        if tab.busy {
            return;
        }
        let Panel::Sql {
            results, workbench, ..
        } = &mut tab.panel
        else {
            return;
        };
        let Some(script) = &workbench.script else {
            return;
        };
        let Some(interruption) = &script.interruption else {
            return;
        };
        let show_rows = std::env::var("TOMATO_RECOVERY_CASE").as_deref() == Ok("results");
        if show_rows {
            workbench.script_view = 1;
        }
        if workbench.failure.is_some() {
            ctx.data_mut(|data| {
                data.insert_temp(
                    Id::new("tomato-capture-viewport"),
                    egui::ViewportId::from_hash_of(("database-error", tab.id.as_str())),
                )
            });
        }

        let proof = serde_json::json!({"successCount":script.success_count,"failCount":script.fail_count,"statements":script.statements.len(),"results":results.len(),"retainedValue":results.first().and_then(|result|result.rows.first()).and_then(|row|row.first()),"connection":interruption.connection,"scriptView":workbench.script_view,"modalError":workbench.failure.is_some(),"inlineError":tab.error.is_some(),"resultContext":workbench.result_context.as_ref().map(|context|context.connection.id.as_str()),"physicalOsInputUsed":false});
        if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from)
            && let Ok(text) = serde_json::to_vec_pretty(&proof)
            && std::fs::write(path.with_extension("recovery.json"), text).is_ok()
        {
            ctx.data_mut(|data| data.insert_temp(Id::new("owned-recovery-ready"), true));
            ctx.request_repaint();
        }
    }
}
