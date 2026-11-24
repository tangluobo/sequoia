//! A committed disk prefix feeds Explorer while the network producer is still running.
use super::*;
use std::{
    io,
    pin::Pin,
    task::{Context as TaskContext, Poll, ready},
};
use tokio::io::{AsyncWrite, AsyncWriteExt};

#[derive(Debug, Default)]
pub(super) struct State {
    value: Mutex<Prefix>,
    changed: Condvar,
}
#[derive(Debug, Default)]
struct Prefix {
    enabled: bool,
    written: u64,
    finished: Option<std::result::Result<(), String>>,
}
impl State {
    fn advance(&self, count: u64) {
        if let Ok(mut state) = self.value.lock() {
            state.written += count;
            self.changed.notify_all();
        }
    }
    pub(super) fn fail(&self, error: String) {
        if let Ok(mut state) = self.value.lock()
            && state.enabled
        {
            state.finished = Some(Err(error));
            self.changed.notify_all();
        }
    }
    #[cfg(any(windows, test))]
    pub(super) fn available(
        &self,
        offset: u64,
        count: u32,
        cancelled: impl Fn() -> bool,
    ) -> Result<u32> {
        let mut state = self
            .value
            .lock()
            .map_err(|_| anyhow::anyhow!("文件流状态锁失效"))?;
        if !state.enabled {
            return Ok(count);
        }
        loop {
            ensure!(!cancelled(), "文件拖放已取消");
            if let Some(Err(error)) = &state.finished {
                anyhow::bail!(error.clone());
            }
            let available = state.written.saturating_sub(offset).min(u64::from(count));
            if available == u64::from(count) || state.finished.is_some() {
                return Ok(available as u32);
            }
            state = self
                .changed
                .wait_timeout(state, std::time::Duration::from_millis(50))
                .map_err(|_| anyhow::anyhow!("文件流等待失败"))?
                .0;
        }
    }
}
pub(crate) struct Writer {
    file: tokio::fs::File,
    asset: Arc<Asset>,
    pending: u64,
    completed: bool,
}
impl Writer {
    pub(crate) async fn new(remote: &str) -> Result<Self> {
        let asset = Asset::prepare(remote).await?;
        let file = tokio::fs::File::create(asset.path()).await?;
        asset
            .stream
            .value
            .lock()
            .map_err(|_| anyhow::anyhow!("文件流状态锁失效"))?
            .enabled = true;
        Ok(Self {
            file,
            asset,
            pending: 0,
            completed: false,
        })
    }
    pub(crate) fn asset(&self) -> Arc<Asset> {
        self.asset.clone()
    }
    pub(crate) async fn finish(mut self) -> Result<Arc<Asset>> {
        self.flush().await?;
        if let Ok(mut state) = self.asset.stream.value.lock() {
            state.finished = Some(Ok(()));
            self.asset.stream.changed.notify_all();
        }
        self.completed = true;
        Ok(self.asset.clone())
    }
    fn flush_prefix(&mut self, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        ready!(Pin::new(&mut self.file).poll_flush(cx))?;
        if self.pending != 0 {
            self.asset.stream.advance(self.pending);
            self.pending = 0;
        }
        Poll::Ready(Ok(()))
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if !self.completed {
            self.asset.stream.fail("文件传输已取消或中断".into());
        }
    }
}
impl AsyncWrite for Writer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Tokio accepts a write before its blocking disk worker has committed it.
        // Expose the previous prefix only after flush confirms that it is readable.
        ready!(self.flush_prefix(cx))?;
        let count = ready!(Pin::new(&mut self.file).poll_write(cx, bytes))?;
        self.pending += count as u64;
        Poll::Ready(Ok(count))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        self.flush_prefix(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        ready!(self.flush_prefix(cx))?;
        Pin::new(&mut self.file).poll_shutdown(cx)
    }
}
