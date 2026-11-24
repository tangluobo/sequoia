//! Inert layout fixtures. No protocol tasks or writes to the shared profile.
use super::*;
impl TomatoApp {
    pub(super) fn preview_alignment(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Missing isolated layout profile"))?;
        anyhow::ensure!(
            profile.is_absolute()
                && profile.file_name().is_some_and(|name| name
                    .to_string_lossy()
                    .starts_with("tomato-layout-fixture-"))
                && self.store.connections.is_empty(),
            "Requires an empty owned layout profile"
        );
        for (kind, name) in [
            ("LOCAL_TERMINAL", "本地终端"),
            ("SSH", "SSH 生产服务器"),
            ("MYSQL", "MySQL 数据库"),
            ("LOCAL_DIRECTORY", "本地目录"),
        ] {
            let mut connection = Connection::new(kind);
            connection.name = name.into();
            self.store.connections.push(connection);
        }
        let state = std::env::var("TOMATO_LAYOUT_STATE").unwrap_or_default();
        let mut connection = Connection::new(if state.starts_with("rocketmq") {
            "ROCKETMQ"
        } else {
            "KAFKA"
        });
        connection.name = if state.starts_with("rocketmq") {
            "RocketMQ 布局验收"
        } else {
            "Kafka 布局验收"
        }
        .into();
        let mut tab = tabs::create_tab(connection, None)?;
        if let Panel::MessageQueue { view, .. } = &mut tab.panel {
            view.page = tabs::mq::Page::Messages;
            view.loaded = true;
            view.topic = "orders.production".into();
            let message = serde_json::json!({
                "topic": "orders.production",
                "msgId": "7F00000100002A9F00000000000004D2",
                "offset": 1234567890,
                "queueOffset": 1234567890,
                "partition": 2,
                "key": "customer-order-20261020-1234567890",
                "timestamp": 1792454400000_i64,
                "headers": [{"key":"trace-id", "value":"0123456789abcdef0123456789abcdef"}, {"key":"来源", "value":"生产环境订单服务"}],
                "body": "{\"orderId\":1234567890,\"status\":\"已支付\"}"
            });
            view.detail = message.to_string();
            view.rows.push(message);
            view.body = "{\"orderId\":1234567890,\"status\":\"已支付\"}".into();
            view.body_view.reset(&view.body, true);
        }
        self.active = Some(tab.id.clone());
        self.tabs.push(tab);
        if state == "picker" {
            self.picker = Some(2);
        }
        if state == "tools" {
            self.section = 1;
        }
        if state.ends_with("narrow") {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900., 900.)));
        }
        Ok(())
    }
}
