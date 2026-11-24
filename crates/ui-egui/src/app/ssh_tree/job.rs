//! Tree commands own a short-lived actor, not a hidden or visible workspace tab.
use super::*;
use std::time::{Duration, Instant};
use tomato_services::terminal::ssh::tools::{Output, Table};
use tomato_services::terminal::{Prompt, Status, Terminal};
pub(in crate::app) struct Job {
    pub terminal: Terminal,
    connection: Connection,
    operation: Option<Operation>,
    pending: Option<u64>,
    deadline: Instant,
    pub auth: Option<Auth>,
}
pub(in crate::app) struct Auth {
    title: String,
    prompts: Vec<Prompt>,
    answers: Vec<String>,
    focus: bool,
}
impl Job {
    pub fn new(
        connection: &Connection,
        operation: Operation,
        runtime: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::start_tools(connection, runtime)?;
        let ctx = ctx.clone();
        terminal.on_output(move || ctx.request_repaint());
        Ok(Self {
            terminal,
            connection: connection.clone(),
            operation: Some(operation),
            pending: None,
            deadline: Instant::now() + Duration::from_secs(120),
            auth: None,
        })
    }
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<anyhow::Result<Table>> {
        match self.terminal.status() {
            Status::Authentication { title, prompts } => {
                if self
                    .auth
                    .as_ref()
                    .is_none_or(|a| a.title != title || a.prompts != prompts)
                {
                    self.auth = Some(Auth {
                        answers: vec![String::new(); prompts.len()],
                        title,
                        prompts,
                        focus: true,
                    });
                }
                return None;
            }
            Status::Failed(error) => return Some(Err(anyhow::anyhow!(error))),
            Status::Exited => return Some(Err(anyhow::anyhow!("SSH 连接已关闭"))),
            Status::Connecting => {}
            Status::Ready => {
                self.auth = None;
                for _ in 0..2 {
                    if self.pending.is_none()
                        && let Some(operation) = self.operation.take()
                    {
                        match self.terminal.tool_request(operation) {
                            Ok(id) => self.pending = Some(id),
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    let Some(reply) = self.terminal.try_tool_reply() else {
                        break;
                    };
                    if Some(reply.id) != self.pending {
                        continue;
                    }
                    self.pending = None;
                    match reply.result {
                        Ok(Output::Table(table)) => return Some(Ok(table)),
                        Ok(Output::Done(_)) => {
                            self.operation = Some(Operation::Query(Section::Docker))
                        }
                        Ok(_) => return Some(Err(anyhow::anyhow!("容器操作返回了不匹配的结果"))),
                        Err(error) => return Some(Err(anyhow::anyhow!(error))),
                    }
                }
            }
        }
        if Instant::now() >= self.deadline {
            return Some(Err(anyhow::anyhow!("容器操作超时，请重试")));
        }
        ctx.request_repaint_after(self.deadline.saturating_duration_since(Instant::now()));
        None
    }
}
impl TomatoApp {
    pub(in crate::app) fn poll_ssh_tree(&mut self, ctx: &egui::Context) {
        self.ssh_hosts
            .retain(|id, _| self.store.connections.iter().any(|c| c.id == *id));
        let mut completed = Vec::new();
        for host in self.ssh_hosts.values_mut() {
            if let Some(result) = host.job.as_mut().and_then(|job| job.poll(ctx)) {
                match result {
                    Ok(table) => {
                        host.credentials = host.job.as_ref().map(|job| job.connection.clone());
                        host.containers = table
                            .rows
                            .iter()
                            .filter_map(|row| {
                                Some(Container {
                                    id: row.first()?.clone(),
                                    name: row.get(2)?.clone(),
                                    running: row.get(3)?.to_ascii_lowercase().starts_with("up"),
                                })
                            })
                            .collect();
                        host.updated = Some(Instant::now());
                    }
                    Err(error) => {
                        host.credentials = None;
                        self.error = Some(format!("{error:#}"));
                    }
                }
                if let Some(job) = host.job.take() {
                    completed.push(job.terminal);
                }
            }
        }
        if let Some(runtime) = &self.runtime {
            for terminal in completed {
                runtime.spawn(terminal.shutdown());
            }
        }
    }
    pub(in crate::app) fn ssh_tree_auth(&mut self, ctx: &egui::Context) {
        let Some(host) = self
            .ssh_hosts
            .values_mut()
            .find(|host| host.job.as_ref().is_some_and(|j| j.auth.is_some()))
        else {
            return;
        };
        let job = host.job.as_mut().unwrap();
        let auth = job.auth.as_mut().unwrap();
        let (mut accept, mut cancel) = (false, false);
        egui::Modal::new(egui::Id::new("ssh-tree-auth")).show(ctx, |ui| {
            ui.set_width(360.);
            ui.heading(&auth.title);
            for (index, prompt) in auth.prompts.iter().enumerate() {
                ui.label(&prompt.label);
                let r =
                    ui.add(TextEdit::singleline(&mut auth.answers[index]).password(!prompt.echo));
                if index == 0 && std::mem::take(&mut auth.focus) {
                    r.request_focus();
                }
                accept |= (r.has_focus() || r.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
            ui.horizontal(|ui| {
                accept |= ui.button("确定").clicked();
                cancel = ui.button("取消").clicked();
            });
        });
        if accept {
            match job.terminal.answer(std::mem::take(&mut auth.answers)) {
                Ok(()) => {
                    job.auth = None;
                    job.deadline = Instant::now() + Duration::from_secs(120);
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    cancel = true;
                }
            }
        }
        if cancel
            && let Some(job) = host.job.take()
            && let Some(runtime) = &self.runtime
        {
            runtime.spawn(job.terminal.shutdown());
        }
    }
}
