use super::*;
use tomato_services::terminal::ssh::tools::{ManagementAction, Operation, Section};
mod job;
#[cfg(test)]
mod tests;
#[derive(Default)]
pub(super) struct Host {
    pub containers: Vec<Container>,
    pub updated: Option<std::time::Instant>,
    pub job: Option<Box<job::Job>>,
    credentials: Option<Connection>,
}
pub(super) struct Container {
    pub id: String,
    pub name: String,
    pub running: bool,
}
impl TomatoApp {
    pub(super) fn cached_ssh_connection(&self, mut connection: Connection) -> Connection {
        if connection.text("password").is_empty()
            && let Some(cached) = self
                .ssh_hosts
                .get(&connection.id)
                .and_then(|h| h.credentials.as_ref())
        {
            let public = |key: &str| key != "password" && key != "savePassword";
            if cached.kind() == connection.kind()
                && cached
                    .options
                    .iter()
                    .filter(|(k, _)| public(k))
                    .eq(connection.options.iter().filter(|(k, _)| public(k)))
            {
                connection.set("password", cached.text("password"));
            }
        }
        connection
    }
    pub(super) fn open_ssh_tool(&mut self, connection: Connection, section: Section) {
        let connection = self.cached_ssh_connection(connection);
        if section == Section::Terminal {
            self.open_connection(connection);
            return;
        }
        let source = format!("ssh:{}:{section:?}", connection.id);
        if let Some(tab) = self.tabs.iter().find(|t| t.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        match tabs::create_ssh_tool_tab(connection, section, runtime.handle()) {
            Ok(mut tab) => {
                terminal_config::apply(&mut tab, &self.settings);
                if let Panel::Terminal { session, .. } = &tab.panel {
                    let ctx = self.context.clone();
                    session.on_output(move || ctx.request_repaint());
                }
                self.active = Some(tab.id.clone());
                self.tabs.push(tab);
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn open_container_inspector(&mut self, connection: Connection, id: String) {
        let connection = self.cached_ssh_connection(connection);
        let source = format!("ssh:{}:inspect:{id}", connection.id);
        if let Some(tab) = self.tabs.iter().find(|t| t.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let name = self
            .ssh_hosts
            .get(&connection.id)
            .and_then(|host| host.containers.iter().find(|c| c.id == id))
            .map_or_else(|| id.clone(), |c| c.name.clone());
        let Some(runtime) = &self.runtime else {
            return;
        };
        match tabs::create_ssh_tool_tab(connection, Section::Docker, runtime.handle()) {
            Ok(mut tab) => {
                tab.source_id = source;
                tab.title = format!("{name} 属性");
                if let Panel::Terminal { view, session, .. } = &mut tab.panel {
                    if let Some(w) = &mut view.ssh {
                        w.inspector_id = Some(id.clone());
                        w.panes.entry(Section::Docker).or_default().refresh = false;
                        w.enqueue(
                            Section::Docker,
                            Operation::Manage(ManagementAction::DockerInspect(id)),
                        );
                    }
                    let ctx = self.context.clone();
                    session.on_output(move || ctx.request_repaint());
                }
                self.active = Some(tab.id.clone());
                self.tabs.push(tab);
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }
    pub(super) fn ssh_tree_request(&mut self, connection: Connection, operation: Operation) {
        if self
            .ssh_hosts
            .get(&connection.id)
            .is_some_and(|host| host.job.is_some())
        {
            return;
        }
        let connection = self.cached_ssh_connection(connection);
        if connection
            .options
            .get("usePassword")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
            && connection.text("password").is_empty()
        {
            self.credential = Some(credentials::Prompt::then(
                connection,
                credentials::Resume::SshTree { operation },
            ));
            return;
        }
        if self
            .ssh_hosts
            .values()
            .filter(|host| host.job.is_some())
            .count()
            >= 8
        {
            self.error = Some("容器操作过多，请等待当前操作完成".into());
            return;
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        match job::Job::new(&connection, operation, runtime.handle(), &self.context) {
            Ok(job) => self.ssh_hosts.entry(connection.id).or_default().job = Some(Box::new(job)),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn container_action(&mut self, connection: Connection, id: String, action: &str) {
        let connection = self.cached_ssh_connection(connection);
        match action {
            "terminal" | "logs" => {
                match tomato_services::terminal::ssh::tools::management::container_terminal(
                    &id,
                    action == "logs",
                ) {
                    Ok(command) => {
                        self.handle_intent(
                            "",
                            Intent::SshCommand {
                                connection,
                                command,
                                title: format!("容器 - {id}"),
                            },
                        );
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "inspect" => {
                self.open_container_inspector(connection, id);
            }
            other => self.ssh_tree_request(
                connection,
                Operation::Manage(ManagementAction::Docker {
                    action: other.into(),
                    ids: vec![id],
                }),
            ),
        }
    }
}
