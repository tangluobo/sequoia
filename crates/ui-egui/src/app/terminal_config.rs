//! Terminal tab actions and live application of legacy preferences.
use super::*;
mod fixture;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Copy,
    Session,
    Global,
}

pub(super) fn menu(ui: &mut Ui, tab: &Tab) -> Option<Action> {
    if super::file_tabs::supports(tab) {
        return None;
    }
    let Panel::Terminal {
        connection, view, ..
    } = &tab.panel
    else {
        return None;
    };
    if !matches!(connection.kind(), "SSH" | "SFTP" | "LOCAL_TERMINAL") {
        return None;
    }
    let terminal = view.ssh.as_ref().is_none_or(|w| w.standalone.is_none());
    if !terminal
        && view.ssh.as_ref().is_some_and(|w| {
            !matches!(
                w.standalone,
                Some(
                    tomato_services::terminal::ssh::tools::Section::Files
                        | tomato_services::terminal::ssh::tools::Section::Ports
                )
            )
        })
    {
        return None;
    }
    let mut action = None;
    if terminal {
        if ui.add(crate::compact_menu::Item::new("复制会话")).clicked() {
            action = Some(Action::Copy);
        }
        ui.separator();
    }
    for (label, value) in [("会话配置", Action::Session), ("全局配置", Action::Global)] {
        if ui.add(crate::compact_menu::Item::new(label)).clicked() {
            action = Some(value);
        }
    }
    if action.is_some() {
        ui.close();
    }
    action
}

pub(super) fn apply(tab: &mut Tab, global: &SettingsState) {
    apply_runtime(tab, global);
    if let Panel::Terminal {
        connection, view, ..
    } = &mut tab.panel
        && let Some(w) = &mut view.ssh
    {
        w.file_view = connection
            .options
            .get("defaultFileViewMode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| match connection.kind() {
                "S3" | "ALIYUN_OSS" | "SFTP" => "ICON",
                "FTP" | "LOCAL_DIRECTORY" => "LIST",
                _ => &global.ssh_default_file_view_mode,
            })
            .to_ascii_uppercase();
    }
}
pub(super) fn apply_runtime(tab: &mut Tab, global: &SettingsState) {
    let Panel::Terminal {
        connection,
        parser,
        view,
        ..
    } = &mut tab.panel
    else {
        return;
    };
    if !matches!(connection.kind(), "SSH" | "SFTP" | "LOCAL_TERMINAL") {
        return;
    }
    let rows = connection
        .options
        .get("scrollbackLines")
        .and_then(serde_json::Value::as_i64)
        .map(|n| n.max(0) as usize)
        .unwrap_or(global.scrollback_lines);
    parser.set_scrollback_limit(rows);
    view.font_size = Some(global.ssh_terminal_font_size);
    view.paused_output = parser.screen().scrollback() > 0;
    if view.selection.as_ref().is_some_and(|s| {
        s.anchor.0 < parser.screen().first_retained_row()
            || s.end.0 < parser.screen().first_retained_row()
    }) {
        view.clear_selection();
    }
}

impl TomatoApp {
    pub(super) fn record_terminal_config_fixture(&mut self) {
        if !self
            .context
            .data(|data| data.get_temp::<bool>(egui::Id::new("ssh-tab-fixture-active")))
            .unwrap_or(false)
        {
            return;
        }
        let mut counts = (0usize, 0usize);
        let mut inspector = None;
        let mut storage_state = None;
        let mut monitor_state = None;
        for tab in &mut self.tabs {
            if let Panel::Terminal { session, view, .. } = &mut tab.panel {
                counts.0 += 1;
                if let Some(w) = &view.ssh
                    && !w.history.is_empty()
                {
                    let value = w.counters.as_ref().map(|c| (c.read, c.written));
                    monitor_state = Some(
                        serde_json::json!({"samples":w.history.len(),"rawRates":w.metrics,"firstTimestamp":w.history.front().map(|s|s.timestamp),"lastTimestamp":w.history.back().map(|s|s.timestamp),"diskReadBytes":value.map(|v|v.0),"diskWrittenBytes":value.map(|v|v.1)}),
                    );
                }
                if let Some(w) = &view.ssh
                    && w.standalone == Some(tomato_services::terminal::ssh::tools::Section::Storage)
                {
                    let rows = w
                        .panes
                        .get(&tomato_services::terminal::ssh::tools::Section::Storage)
                        .and_then(|p| p.table.as_ref())
                        .map(|t| t.rows.as_slice());
                    let mut flags = if rows.is_some() { 1u8 } else { 0 };
                    if rows.is_some_and(|r| {
                        r.iter()
                            .any(|v| v.first().is_some_and(|s| s == "/dev/sdb1"))
                    }) {
                        flags |= 2;
                    }
                    if w.storage.roots.iter().any(|n| n.group == "vg_data") {
                        flags |= 4;
                    }
                    if rows.is_some_and(|r| {
                        r.iter().any(|v| {
                            v.first().is_some_and(|s| s == "/dev/data/lv_data")
                                && v.get(2).is_some_and(|s| s == "2.00 GiB")
                        })
                    }) {
                        flags |= 8;
                    }
                    if rows.is_some_and(|r| {
                        r.iter()
                            .any(|v| v.get(5).is_some_and(|s| s == "/fixture-data"))
                    }) {
                        flags |= 16;
                    }
                    if w.storage.notice.is_some() {
                        flags |= 32;
                    }
                    if w.panes
                        .contains_key(&tomato_services::terminal::ssh::tools::Section::Files)
                    {
                        flags |= 64;
                    }
                    storage_state = Some((
                        flags,
                        w.busy(tomato_services::terminal::ssh::tools::Section::Storage),
                    ));
                }
                if session.status() == tomato_services::terminal::Status::Ready {
                    counts.1 += 1;
                }
                if let Some(w) = &view.ssh
                    && let Some(value) = &w.inspector
                {
                    inspector = Some((
                        value.limits.memory,
                        value.policy.clone(),
                        w.busy(tomato_services::terminal::ssh::tools::Section::Docker),
                    ));
                }
            }
        }
        let tree = self
            .store
            .connections
            .first()
            .and_then(|c| self.ssh_hosts.get(&c.id))
            .map(|h| {
                (
                    h.containers.first().map(|c| c.running),
                    h.job.is_some(),
                    counts.0,
                )
            });
        self.context.data_mut(|data| {
            if let Some(value) = monitor_state {
                data.insert_temp(egui::Id::new("monitor-fixture-state"), value);
            }
            if let Some(value) = storage_state {
                data.insert_temp(egui::Id::new("storage-fixture-state"), value);
            }
            data.insert_temp(egui::Id::new("ssh-tab-fixture-ready"), counts.1 > 0);
            data.insert_temp(egui::Id::new("ssh-tab-fixture-counts"), counts);
            if let Some(value) = inspector {
                data.insert_temp(egui::Id::new("docker-fixture-inspection"), value);
            }
            if let Some(value) = tree {
                data.insert_temp(egui::Id::new("docker-tree-fixture-state"), value);
            }
        });
    }
    pub(super) fn terminal_config_fixture(&mut self) {
        if self.store.connections.len() != 1
            || !self.store.connections.iter().all(|c| {
                c.kind() == "SSH"
                    && c.text("host") == "127.0.0.1"
                    && c.text("username") == "fixture"
            })
        {
            return;
        }
        if fixture::install(&self.context) {
            self.quit_approved = true;
            if std::env::var("TOMATO_SSH_TAB_FIXTURE").as_deref() == Ok("monitor") {
                if let Some(Tab {
                    panel: Panel::Terminal { view, .. },
                    ..
                }) = self.tabs.last_mut()
                    && let Some(w) = &mut view.ssh
                {
                    w.visible.clear();
                    w.open(tomato_services::terminal::ssh::tools::Section::Monitor);
                }
                return;
            }
            if std::env::var("TOMATO_SSH_TAB_FIXTURE").as_deref() == Ok("ports") {
                self.open_ssh_tool(
                    self.store.connections[0].clone(),
                    tomato_services::terminal::ssh::tools::Section::Ports,
                );
                return;
            }
            if std::env::var("TOMATO_SSH_TAB_FIXTURE").is_ok_and(|m| m.starts_with("storage")) {
                self.open_ssh_tool(
                    self.store.connections[0].clone(),
                    tomato_services::terminal::ssh::tools::Section::Storage,
                );
                return;
            }
            if std::env::var("TOMATO_SSH_TAB_FIXTURE").as_deref() == Ok("docker") {
                self.open_container_inspector(self.store.connections[0].clone(), "web".into());
                return;
            }
            if std::env::var("TOMATO_SSH_TAB_FIXTURE").as_deref() == Ok("docker_tree") {
                self.store.connections[0].options.remove("password");
                let c = self.store.connections[0].clone();
                self.collapsed.remove(&format!("ssh:{}:Docker", c.id));
                self.ssh_tree_request(
                    c,
                    tomato_services::terminal::ssh::tools::Operation::Query(
                        tomato_services::terminal::ssh::tools::Section::Docker,
                    ),
                );
            }
            if let Some(Tab {
                panel: Panel::Terminal { view, .. },
                ..
            }) = self.tabs.last_mut()
                && let Some(w) = &mut view.ssh
            {
                w.visible.clear();
            }
        }
    }
    pub(super) fn terminal_tab_action(&mut self, id: &str, action: Action) {
        match action {
            Action::Session => self.open_session_config(id),
            Action::Global => {
                self.section = 4;
                self.settings_page = super::settings::Page::Ssh;
            }
            Action::Copy => {
                let Some(Panel::Terminal { connection, .. }) =
                    self.tabs.iter().find(|t| t.id == id).map(|t| &t.panel)
                else {
                    return;
                };
                // A duplicate starts a fresh shell, including for a container tab.
                // Current credentials/settings come from the shared connection.
                let next = self
                    .store
                    .connections
                    .iter()
                    .find(|c| c.id == connection.id)
                    .unwrap_or(connection)
                    .clone();
                self.open_connection(next);
            }
        }
    }
}
