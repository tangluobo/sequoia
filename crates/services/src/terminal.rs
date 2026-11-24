//! Terminal transport adapter: local PTY and native SSH share one UI contract.
mod desktop;
mod local;
pub mod ssh;
use anyhow::Result;
pub use desktop::launch_desktop;
use tomato_core::Connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub label: String,
    pub echo: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Ready,
    Authentication { title: String, prompts: Vec<Prompt> },
    Exited,
    Failed(String),
}
pub struct Terminal {
    backend: Backend,
}
enum Backend {
    Local(local::LocalTerminal),
    Ssh(ssh::RemoteTerminal),
    Files(crate::files::session::FileSession),
}
impl Terminal {
    /// Stop actors before awaiting their upload cleanup; ConPTY teardown can
    /// flush output synchronously, so release it on the blocking pool.
    pub async fn shutdown(mut self) {
        match &mut self.backend {
            Backend::Files(session) => session.stop().await,
            Backend::Ssh(session) => session.stop().await,
            Backend::Local(_) => {
                let _ = tokio::task::spawn_blocking(move || drop(self)).await;
            }
        }
    }
    pub fn start_tools(connection: &Connection, runtime: &tokio::runtime::Handle) -> Result<Self> {
        if !matches!(connection.kind(), "SSH" | "SFTP") {
            return Self::start(connection, runtime);
        }
        Ok(Self {
            backend: Backend::Ssh(ssh::RemoteTerminal::start_mode(
                runtime,
                connection.clone(),
                tomato_core::Store::default_path().with_file_name("known_hosts"),
                false,
            )?),
        })
    }
    pub fn start(connection: &Connection, runtime: &tokio::runtime::Handle) -> Result<Self> {
        let backend = if matches!(connection.kind(), "SSH" | "SFTP") {
            Backend::Ssh(ssh::RemoteTerminal::start(
                runtime,
                connection.clone(),
                tomato_core::Store::default_path().with_file_name("known_hosts"),
            )?)
        } else if matches!(
            connection.kind(),
            "LOCAL_DIRECTORY" | "FTP" | "S3" | "ALIYUN_OSS"
        ) {
            Backend::Files(crate::files::session::FileSession::start(
                runtime,
                connection.clone(),
            ))
        } else {
            Backend::Local(local::LocalTerminal::start(connection)?)
        };
        Ok(Self { backend })
    }
    pub fn from_file_session(session: crate::files::session::FileSession) -> Self {
        Self {
            backend: Backend::Files(session),
        }
    }
    pub async fn stop_file_session(&mut self) {
        if let Backend::Files(session) = &mut self.backend {
            session.stop().await;
        }
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        match &self.backend {
            Backend::Local(t) => t.on_output(callback),
            Backend::Ssh(t) => t.on_output(callback),
            Backend::Files(t) => t.on_output(callback),
        }
    }
    pub fn try_read(&mut self) -> Option<Vec<u8>> {
        match &mut self.backend {
            Backend::Local(t) => t.output.try_recv().ok(),
            Backend::Ssh(t) => t.output.try_recv().ok(),
            Backend::Files(_) => None,
        }
    }
    pub fn write(&self, text: &str) -> Result<()> {
        match &self.backend {
            Backend::Local(t) => t.write(text),
            Backend::Ssh(t) => t.write(text),
            Backend::Files(_) => anyhow::bail!("文件浏览器没有终端输入"),
        }
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        match &self.backend {
            Backend::Local(t) => t.resize(rows, cols),
            Backend::Ssh(t) => t.resize(rows, cols),
            Backend::Files(_) => Ok(()),
        }
    }
    pub fn status(&mut self) -> Status {
        match &mut self.backend {
            Backend::Local(t) => {
                if t.exited() {
                    Status::Exited
                } else {
                    Status::Ready
                }
            }
            Backend::Ssh(t) => t.status(),
            Backend::Files(t) => t.status(),
        }
    }
    pub fn answer(&self, answers: Vec<String>) -> Result<()> {
        match &self.backend {
            Backend::Ssh(t) => t.answer(answers),
            _ => anyhow::bail!("本地终端不需要 SSH 认证"),
        }
    }
    pub fn tool_request(&mut self, operation: ssh::tools::Operation) -> Result<u64> {
        match &mut self.backend {
            Backend::Ssh(t) => t.request(operation),
            Backend::Files(t) => t.request(operation),
            _ => anyhow::bail!("此功能需要 SSH 会话"),
        }
    }
    pub fn document_client(&self) -> Option<crate::files::document::Client> {
        match &self.backend {
            Backend::Ssh(session) => Some(session.document_client()),
            Backend::Files(session) => Some(session.document_client()),
            _ => None,
        }
    }
    pub fn try_tool_reply(&mut self) -> Option<ssh::tools::Reply> {
        match &mut self.backend {
            Backend::Ssh(t) => t.try_reply(),
            Backend::Files(t) => t.try_reply(),
            _ => None,
        }
    }
    pub fn transfer_state(&self) -> ssh::zmodem::TransferState {
        match &self.backend {
            Backend::Ssh(t) => t.transfer_state(),
            _ => ssh::zmodem::TransferState::Idle,
        }
    }
    pub fn prepare_upload(&self) -> Result<()> {
        match &self.backend {
            Backend::Ssh(t) => t.prepare_upload(),
            _ => Ok(()),
        }
    }
    pub fn transfer_choice(&self, choice: ssh::zmodem::Choice) -> Result<()> {
        match &self.backend {
            Backend::Ssh(t) => t.transfer_choice(choice),
            _ => anyhow::bail!("ZModem 需要 SSH 会话"),
        }
    }
}
