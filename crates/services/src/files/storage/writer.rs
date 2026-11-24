//! Async close/abort plus a Drop fallback for actors aborted during a write.
use super::*;
use crate::files::object_copy::Job;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
struct Cleanup {
    active: AtomicUsize,
    notify: tokio::sync::Notify,
}
fn cleanup() -> &'static Cleanup {
    static STATE: OnceLock<Cleanup> = OnceLock::new();
    STATE.get_or_init(|| Cleanup {
        active: AtomicUsize::new(0),
        notify: tokio::sync::Notify::new(),
    })
}
pub(super) async fn wait_cleanup() {
    let state = cleanup();
    loop {
        let notified = state.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if state.active.load(Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}
pub(super) struct UploadWriter {
    writer: Option<opendal::Writer>,
    job: Option<Arc<Job>>,
}
impl UploadWriter {
    pub(super) fn new(writer: opendal::Writer, job: Option<Arc<Job>>) -> Self {
        Self {
            writer: Some(writer),
            job,
        }
    }
    pub(super) fn get(&mut self) -> &mut opendal::Writer {
        self.writer.as_mut().expect("open object writer")
    }
    pub(super) async fn close(&mut self) -> Result<()> {
        self.get().close().await?;
        self.writer = None;
        Ok(())
    }
    pub(super) async fn abort(&mut self) {
        if let Some(writer) = &mut self.writer {
            let _ = tokio::time::timeout(Duration::from_secs(5), writer.abort()).await;
        }
        self.writer = None;
    }
}
struct Completion(Option<Arc<Job>>);
impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(job) = &self.0 {
            job.cleanup_finished();
        }
        cleanup().active.fetch_sub(1, Ordering::AcqRel);
        cleanup().notify.notify_waiters();
    }
}
impl Drop for UploadWriter {
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            cleanup().active.fetch_add(1, Ordering::AcqRel);
            if let Some(job) = &self.job {
                job.cleanup_started();
            }
            let completion = Completion(self.job.clone());
            runtime.spawn(async move {
                let _completion = completion;
                let _ = tokio::time::timeout(Duration::from_secs(5), writer.abort()).await;
            });
        }
    }
}
