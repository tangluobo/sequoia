//! Capture fixtures are limited to an empty disposable profile and never use saved hosts.
use super::*;
impl TomatoApp {
    pub(super) fn preview_device(&mut self, kind: &str) -> anyhow::Result<()> {
        let profile = PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR")
                .ok_or_else(|| anyhow::anyhow!("需要隔离验收目录"))?,
        );
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-device-fixture-"))
                && self.store.connections.is_empty(),
            "设备验收必须使用独立空配置"
        );
        if kind == "adb_config" {
            let mut connection = Connection::new("ADB");
            connection.name = "Android 设备".into();
            connection.set("host", "192.168.1.100");
            self.edit_connection(connection);
        } else if matches!(kind, "adb" | "adb_png" | "adb_window") {
            let port = std::fs::read_to_string(profile.join("adb-fixture.port"))?
                .trim()
                .parse::<u16>()?;
            anyhow::ensure!(port != 0, "ADB 验收端口无效");
            let mut connection = Connection::new("ADB");
            connection.name = "隔离 Android ADB".into();
            connection.set("host", "127.0.0.1");
            connection.set("port", port);
            if kind == "adb_png" {
                connection.set("adbDisplayMode", "PNG");
            }
            connection.set("fullscreen", kind == "adb_window");
            self.open_connection(connection);
            if kind != "adb" {
                self.context
                    .data_mut(|data| data.insert_temp(egui::Id::new("adb-fixture-control"), true));
            }
            if kind == "adb_window"
                && let Some(tab) = self.tabs.last()
            {
                self.context.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("tomato-capture-viewport"),
                        rdp_windows::viewport(&tab.id),
                    )
                });
            }
        } else {
            self.section = 1;
            self.open_tool("port_mapping", "端口映射");
            let mut host = Connection::new("SSH");
            host.name = "隔离 SSH 主机".into();
            host.set("host", "127.0.0.1");
            host.set("port", 22);
            let host_id = host.id.clone();
            self.store.connections.push(host);
            if let Some(tab) = self.tabs.last_mut()
                && let Panel::Tool(tool) = &mut tab.panel
            {
                let mut state = crate::tabs::port_mapping::State::new(
                    profile.join("port-mappings.json"),
                    self.runtime.as_ref().map(|r| r.handle().clone()),
                    profile.join("known_hosts"),
                );
                for direction in ["L", "R"] {
                    state
                        .mappings
                        .push(tomato_services::terminal::ssh::forwarding::Mapping {
                            name: format!("隔离映射 {direction}"),
                            ssh_host_id: host_id.clone(),
                            direction: direction.into(),
                            local_port: 8080,
                            remote_port: 80,
                            ..Default::default()
                        });
                }
                if kind == "port_mapping_edit" {
                    state.editor = Some(crate::tabs::port_mapping::Editor {
                        mapping: state.mappings[0].clone(),
                        local: "8080".into(),
                        remote: "80".into(),
                        editing: Some(0),
                        focus: true,
                    });
                }
                tool.port_mapping = Some(Box::new(state));
            }
        }
        Ok(())
    }
    pub(super) fn poll_preview_device(&self) {
        let key = egui::Id::new("adb-fixture-control");
        if !self
            .context
            .data(|data| data.get_temp::<bool>(key).unwrap_or(false))
        {
            return;
        }
        if let Some(tab) = self.tabs.last()
            && let Panel::NativeDesktop {
                session: Some(session),
                ..
            } = &tab.panel
            && session.status() == tomato_services::remote_desktop::Status::Ready
            && session
                .send(tomato_services::remote_desktop::Input::AndroidKey(3))
                .is_ok()
        {
            self.context.data_mut(|data| data.insert_temp(key, false));
        }
    }
}
