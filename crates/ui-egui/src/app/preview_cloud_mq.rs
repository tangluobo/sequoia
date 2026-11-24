//! Native captures use only explicitly owned loopback servers and a disposable empty profile.
use super::*;
impl TomatoApp {
    pub(super) fn preview_cloud_mq(&mut self, mode: &str) -> anyhow::Result<()> {
        let profile = PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR")
                .ok_or_else(|| anyhow::anyhow!("Missing isolated profile"))?,
        );
        anyhow::ensure!(
            profile.is_absolute()
                && profile
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("tomato-cloud-mq-fixture-"))
                && self.store.connections.is_empty(),
            "Requires an empty disposable cloud/MQ profile"
        );
        let ready: serde_json::Value =
            serde_json::from_slice(&std::fs::read(profile.join("ready.json"))?)?;
        let cloud = mode.starts_with("cloud_");
        let kind = if cloud {
            "TENCENT_CLOUD"
        } else if mode.starts_with("kafka_") {
            "KAFKA"
        } else {
            "ROCKETMQ"
        };
        let mut c = Connection::new(kind);
        c.name = format!("隔离 {kind}");
        if cloud {
            let endpoint = ready["endpoint"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing fixture endpoint"))?;
            let address: std::net::SocketAddr = endpoint
                .strip_prefix("http://")
                .ok_or_else(|| anyhow::anyhow!("Requires HTTP loopback"))?
                .parse()?;
            anyhow::ensure!(address.ip().is_loopback(), "Requires literal loopback");
            c.set("username", "fixture-id");
            c.set("password", "fixture-secret");
            c.set("cloudApiEndpoint", format!("{endpoint}/cloud"));
            c.set("acmeDirectory", format!("{endpoint}/acme/directory"));
            c.set(
                "certificateDnsServer",
                ready["dns"].as_str().unwrap_or_default(),
            );
            let mut storage = Connection::new("S3");
            storage.name = "隔离证书存储".into();
            storage.set("endpoint", endpoint);
            storage.set("username", "fixture-storage");
            storage.set("password", "fixture-storage-secret");
            storage.set("region", "us-east-1");
            storage.set("pathStyleAccess", true);
            self.store.connections.push(storage);
        } else {
            let address: std::net::SocketAddr = ready
                [if kind == "KAFKA" { "kafka" } else { "rocketmq" }]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing broker fixture"))?
            .parse()?;
            anyhow::ensure!(address.ip().is_loopback(), "Requires loopback broker");
            c.set("host", address.ip().to_string());
            c.set("port", address.port());
        }
        self.store.connections.push(c.clone());
        self.remote_open(c.clone(), false);
        let mut tab = tabs::create_tab(c, self.runtime.as_ref().map(|r| r.handle()))?;
        match &mut tab.panel {
            Panel::Cloud { view, .. } => {
                view.ddns_path = profile.join("ddns.json");
                view.page = tabs::cloud::Page::Records("owned.test".into());
                if mode == "cloud_certificate" {
                    view.certificate("owned.test", "owned.test", "");
                }
            }
            Panel::MessageQueue { view, .. } if mode.ends_with("_groups") => {
                view.page = tabs::mq::Page::Groups;
            }
            Panel::MessageQueue { view, .. } if mode.ends_with("_batch") => {
                view.page = tabs::mq::Page::Groups;
                view.loaded = true;
                let groups = [
                    "tomato_fixture_first",
                    "tomato_fixture_second",
                    "tomato_fixture_third",
                ];
                view.rows = groups
                    .iter()
                    .map(|name| serde_json::json!({"group":name}))
                    .collect();
                view.group_selection.rows.extend([0, 1]);
                view.confirm = Some(tomato_services::mq::Action::DeleteGroups {
                    groups: groups[..2].iter().map(|name| (*name).to_owned()).collect(),
                });
            }
            Panel::MessageQueue { view, .. } if mode.ends_with("_send") => {
                view.page = tabs::mq::Page::Messages;
                view.query = tabs::mq::QueryPage::Send;
                view.topic = ready["topic"].as_str().unwrap_or("tomato_capture").into();
                view.loaded = true;
            }
            _ => {}
        }
        self.active = Some(tab.id.clone());
        self.tabs.push(tab);
        Ok(())
    }
}
