use super::*;
impl TomatoApp {
    pub(super) fn configure_server(
        &mut self,
        id: &str,
        config: &tomato_core::server::ServerConfig,
    ) -> bool {
        let result = self.persist_server_config(id, config);
        self.server_save_result(id, result.is_ok());
        if let Err(error) = result {
            self.error = Some(format!("{error:#}"));
            return false;
        }
        true
    }
    pub(super) fn start_configured_server(
        &mut self,
        id: &str,
        config: tomato_core::server::ServerConfig,
    ) {
        if !self.configure_server(id, &config) {
            return;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.busy = true;
            tab.error = None;
        }
        if let Some(runtime) = &self.runtime {
            let send = self.server_sender.clone();
            let id = id.to_owned();
            let ctx = self.context.clone();
            runtime.spawn(async move {
                let result = tomato_services::servers::start(config).await;
                let _ = send.send((id, result));
                ctx.request_repaint();
            });
        }
    }
    fn server_save_result(&mut self, id: &str, saved: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let Panel::Server(view) = &mut tab.panel
        {
            view.changed = !saved;
            view.save_failed = !saved;
        }
    }
    fn persist_server_config(
        &mut self,
        id: &str,
        config: &tomato_core::server::ServerConfig,
    ) -> anyhow::Result<()> {
        config.validate()?;
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else {
            return Ok(());
        };
        let source = tab.source_id.clone();
        if source.starts_with("server:") {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id)
                && let Panel::Server(view) = &tab.panel
                && let Some(server) = &view.running
            {
                server.update_config(config.clone())?;
            }
            return Ok(());
        }
        anyhow::ensure!(
            !self.load_failed,
            "配置加载失败，保存已禁用。请修复原配置后重启。"
        );
        let mut store = self.store.clone();
        let connection = store
            .connections
            .iter_mut()
            .find(|c| c.id == source)
            .ok_or_else(|| anyhow::anyhow!("服务器连接已被删除"))?;
        connection.set("serverConfig", serde_json::to_value(config)?);
        self.repository.save(&store)?;
        self.store = store;
        if let Some(tab) = self.tabs.iter().find(|t| t.id == id)
            && let Panel::Server(view) = &tab.panel
            && let Some(server) = &view.running
        {
            server.update_config(config.clone())?;
        }
        Ok(())
    }
    pub(super) fn flush_server_config(&mut self, id: &str) -> bool {
        let config = self.tabs.iter().find(|t| t.id == id).and_then(|tab| {
            let Panel::Server(view) = &tab.panel else {
                return None;
            };
            view.changed.then(|| {
                let mut config = view.config.clone();
                config.port = view.port.parse::<u16>().ok()?;
                Some(config)
            })
        });
        let Some(config) = config else {
            return true;
        };
        let Some(config) = config else {
            self.error = Some("端口必须为 0–65535 的整数".into());
            return false;
        };
        if let Err(error) = self.persist_server_config(id, &config) {
            self.error = Some(format!("{error:#}"));
            return false;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
            && let Panel::Server(view) = &mut tab.panel
        {
            view.config = config;
            view.changed = false;
            view.save_failed = false;
        }
        true
    }
}
