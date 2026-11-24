//! Shared command use cases. Session actors own sockets, hooks and bounded queues.
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod transport;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tomato_core::input_share::Node;
pub const PORT: u16 = 24819;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Start { port: u16 },
    Join { address: String, code: String },
    Scan,
    Order { ids: Vec<String> },
    Resume,
    Release,
    Status,
    Stop,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub local_id: String,
    pub active: String,
    pub running: bool,
    pub hosting: bool,
    pub port: u16,
    pub code: String,
    pub nodes: Vec<Node>,
    pub discovered: Vec<String>,
    pub error: String,
}
#[derive(Default)]
pub struct Manager {
    #[cfg(windows)]
    session: Option<session::Session>,
}
impl Manager {
    pub async fn execute(&mut self, action: Action) -> Result<Snapshot> {
        #[cfg(windows)]
        {
            match action {
                Action::Start { port } => {
                    if self.session.as_ref().is_some_and(|s| !s.snapshot().running) {
                        self.session = None;
                    }
                    anyhow::ensure!(self.session.is_none(), "请先停止当前共享服务");
                    self.session = Some(session::Session::start(port, None, None).await?);
                }
                Action::Join { address, code } => {
                    anyhow::ensure!(
                        self.session.as_ref().is_none_or(|s| {
                            let snapshot = s.snapshot();
                            !snapshot.running || (snapshot.hosting && snapshot.nodes.len() == 1)
                        }),
                        "请先停止当前多人共享会话"
                    );
                    let avoid = self.session.as_ref().map(|s| s.snapshot().local_id);
                    let next = session::Session::start(PORT, Some((address, code)), avoid).await?;
                    if let Some(old) = self.session.take() {
                        old.stop().await;
                    }
                    self.session = Some(next);
                }
                Action::Stop => {
                    if let Some(old) = self.session.take() {
                        old.stop().await;
                    }
                }
                Action::Scan => {
                    return Ok(Snapshot {
                        discovered: transport::scan().await?,
                        ..self.snapshot()
                    });
                }
                Action::Status => {}
                other => {
                    let session = self
                        .session
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请先启动或加入共享服务"))?;
                    session.action(other).await?;
                }
            }
            Ok(self.snapshot())
        }
        #[cfg(not(windows))]
        {
            let _ = action;
            anyhow::bail!("键鼠共享的系统输入适配器目前仅支持 Windows")
        }
    }
    #[cfg(windows)]
    fn snapshot(&self) -> Snapshot {
        self.session
            .as_ref()
            .map_or_else(Snapshot::default, |s| s.snapshot())
    }
}
