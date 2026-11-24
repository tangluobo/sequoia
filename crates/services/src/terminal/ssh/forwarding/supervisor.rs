//! A mapping owns one cancellable reconnect loop, independent of visible tabs.
use super::*;
use std::sync::LazyLock;
use tokio::sync::{Semaphore, SemaphorePermit};

static CONNECTING: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(16));
pub(super) async fn permit() -> Result<SemaphorePermit<'static>> {
    Ok(CONNECTING.acquire().await?)
}

#[derive(Clone, Debug)]
pub enum ForwardState {
    Connecting,
    Running,
    Retrying {
        attempt: u32,
        delay: Duration,
        error: String,
    },
    Stopped,
}
pub struct Forward {
    task: tokio::task::JoinHandle<()>,
    state: Arc<parking_lot::Mutex<ForwardState>>,
}
impl Forward {
    /// Preserve callers that need the first listener to be ready before returning.
    pub async fn open(mapping: Mapping, host: Connection, known_hosts: PathBuf) -> Result<Self> {
        let running = Running::open(mapping.clone(), host.clone(), known_hosts.clone()).await?;
        Ok(Self::spawn(
            mapping,
            host,
            known_hosts,
            Some(running),
            || {},
        ))
    }
    /// Start a desired-running mapping, including recovery from initial failures.
    pub fn start(
        mapping: Mapping,
        host: Connection,
        known_hosts: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        mapping.validate()?;
        ensure!(
            matches!(host.kind(), "SSH" | "SFTP") && host.id == mapping.ssh_host_id,
            "引用的 SSH 主机不存在"
        );
        Ok(Self::spawn(mapping, host, known_hosts, None, wake))
    }
    fn spawn(
        mapping: Mapping,
        host: Connection,
        known_hosts: PathBuf,
        mut initial: Option<Running>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let state = Arc::new(parking_lot::Mutex::new(if initial.is_some() {
            ForwardState::Running
        } else {
            ForwardState::Connecting
        }));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            let mut attempt = 0u32;
            loop {
                let opened = if let Some(running) = initial.take() {
                    Ok(running)
                } else {
                    *shared.lock() = ForwardState::Connecting;
                    wake();
                    match tokio::time::timeout(
                        Duration::from_secs(60),
                        Running::open(mapping.clone(), host.clone(), known_hosts.clone()),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(anyhow::anyhow!("连接端口映射超时")),
                    }
                };
                let error = match opened {
                    Ok(mut running) => {
                        *shared.lock() = ForwardState::Running;
                        wake();
                        let since = tokio::time::Instant::now();
                        let result = (&mut running.task).await;
                        // Close the old socket and all old channel jobs before rebinding.
                        drop(running);
                        if since.elapsed() >= Duration::from_secs(30) {
                            attempt = 0;
                        }
                        match result {
                            Ok(Err(error)) => format!("{error:#}"),
                            Ok(Ok(())) => "转发任务意外结束".into(),
                            Err(_) => "转发任务意外终止".into(),
                        }
                    }
                    Err(error) => format!("{error:#}"),
                };
                attempt = attempt.saturating_add(1);
                let delay = retry_delay(attempt);
                *shared.lock() = ForwardState::Retrying {
                    attempt,
                    delay,
                    error: error.chars().take(1024).collect(),
                };
                wake();
                tokio::time::sleep(delay).await;
            }
        });
        Self { task, state }
    }
    pub fn is_active(&self) -> bool {
        !self.task.is_finished()
    }
    pub fn is_running(&self) -> bool {
        self.is_active() && matches!(*self.state.lock(), ForwardState::Running)
    }
    pub fn state(&self) -> ForwardState {
        if self.is_active() {
            self.state.lock().clone()
        } else {
            ForwardState::Stopped
        }
    }
}
impl Drop for Forward {
    fn drop(&mut self) {
        // Abort also cancels an in-progress handshake or backoff sleep. No join on UI close.
        self.task.abort();
    }
}
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(
        1u64.checked_shl(attempt.saturating_sub(1).min(5))
            .unwrap_or(30)
            .min(30),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_backoff_is_bounded_and_cannot_overflow() {
        let delays: Vec<_> = [1, 2, 3, 4, 5, 6, 100, u32::MAX]
            .map(|n| retry_delay(n).as_secs())
            .into();
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
    }
}
