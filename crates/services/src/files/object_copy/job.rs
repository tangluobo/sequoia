use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Scanning,
    Copying,
    Finished,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub phase: Phase,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub current: String,
    pub transferred: u64,
    pub file_bytes: Option<u64>,
    pub transfer_stage: &'static str,
    pub total_bytes: Option<u64>,
    pub completed_bytes: u64,
    pub errors: Vec<(String, usize)>,
    pub other_errors: usize,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Scanning,
            total: 0,
            succeeded: 0,
            failed: 0,
            current: String::new(),
            transferred: 0,
            file_bytes: None,
            transfer_stage: "",
            total_bytes: None,
            completed_bytes: 0,
            errors: Vec::new(),
            other_errors: 0,
        }
    }
}
type Wake = Arc<dyn Fn() + Send + Sync>;
struct State {
    snapshot: Arc<Snapshot>,
    last_wake: Instant,
}
pub struct Job {
    state: Mutex<State>,
    cancelled: AtomicBool,
    cancel_notify: tokio::sync::Notify,
    cleanup: AtomicUsize,
    cleanup_notify: tokio::sync::Notify,
    wake: Mutex<Option<Wake>>,
}
impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyJob")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}
impl Default for Job {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                snapshot: Arc::new(Snapshot::default()),
                last_wake: Instant::now(),
            }),
            cancelled: AtomicBool::new(false),
            cancel_notify: tokio::sync::Notify::new(),
            cleanup: AtomicUsize::new(0),
            cleanup_notify: tokio::sync::Notify::new(),
            wake: Mutex::new(None),
        }
    }
}
impl Job {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.state
            .lock()
            .map(|s| s.snapshot.clone())
            .unwrap_or_else(|_| {
                Arc::new(Snapshot {
                    phase: Phase::Failed,
                    current: "复制任务状态不可用".into(),
                    ..Default::default()
                })
            })
    }
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut wake) = self.wake.lock() {
            *wake = Some(Arc::new(callback));
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
        self.wake();
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub async fn cancelled(&self) {
        loop {
            let notified = self.cancel_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
    pub(crate) fn cleanup_started(&self) {
        self.cleanup.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn cleanup_finished(&self) {
        self.cleanup.fetch_sub(1, Ordering::AcqRel);
        self.cleanup_notify.notify_waiters();
    }
    pub async fn wait_cleanup(&self) {
        loop {
            let notified = self.cleanup_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.cleanup.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
    fn wake(&self) {
        let wake = self.wake.lock().ok().and_then(|w| w.clone());
        if let Some(wake) = wake {
            wake();
        }
    }
    fn update(&self, force: bool, mutate: impl FnOnce(&mut Snapshot)) {
        let notify = if let Ok(mut state) = self.state.lock() {
            mutate(Arc::make_mut(&mut state.snapshot));
            if force || state.last_wake.elapsed() >= Duration::from_millis(200) {
                state.last_wake = Instant::now();
                true
            } else {
                false
            }
        } else {
            false
        };
        if notify {
            self.wake();
        }
    }
    pub fn found(&self, total: usize, name: &str) {
        self.update(false, |s| {
            s.total = total;
            s.current = short(name, 512);
        });
    }
    pub fn waiting(&self) {
        self.update(true, |s| s.phase = Phase::Waiting);
    }
    pub fn scanning(&self) {
        self.update(true, |s| {
            s.phase = Phase::Scanning;
            s.current.clear();
        });
    }
    pub fn begin_file(&self, total: usize, name: &str, size: Option<u64>) {
        self.update(true, |s| {
            s.phase = Phase::Copying;
            s.total = total;
            s.current = short(name, 512);
            s.transferred = 0;
            s.file_bytes = size;
            s.transfer_stage = "";
        });
    }
    pub fn transfer_stage(&self, stage: &'static str, size: Option<u64>) {
        self.update(true, |s| {
            s.transfer_stage = stage;
            s.file_bytes = size;
            s.transferred = 0;
        });
    }
    pub fn transferred(&self, bytes: u64) {
        self.update(false, |s| s.transferred = bytes);
    }
    pub fn inventory(&self, total: usize, bytes: u64) {
        self.update(true, |s| {
            s.total = total;
            s.total_bytes = Some(bytes);
        });
    }
    pub fn account_bytes(&self, bytes: u64) {
        self.update(false, |s| {
            s.completed_bytes = s.completed_bytes.saturating_add(bytes);
            s.transferred = 0;
        });
    }
    pub fn completed_file(&self, error: Option<&str>) {
        self.update(true, |s| {
            if let Some(error) = error {
                s.failed = s.failed.saturating_add(1);
                let text = short(error, 4096);
                if let Some((_, count)) = s.errors.iter_mut().find(|(e, _)| *e == text) {
                    *count = count.saturating_add(1);
                } else if s.errors.len() < 5 {
                    s.errors.push((text, 1));
                } else {
                    s.other_errors = s.other_errors.saturating_add(1);
                }
            } else {
                s.succeeded = s.succeeded.saturating_add(1);
            }
        });
    }
    pub fn finish(&self) {
        self.update(true, |s| {
            s.phase = if self.is_cancelled() {
                Phase::Cancelled
            } else {
                Phase::Finished
            }
        });
    }
    pub fn fail(&self, error: &str) {
        self.update(true, |s| {
            s.phase = Phase::Failed;
            s.current = short(error, 4096);
        });
    }
}
fn short(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_owned()
    } else {
        format!("{}…", &text[..text.floor_char_boundary(limit)])
    }
}
/// UI ownership is separate from worker references: closing a workspace always
/// cancels its running copy, even while the actor still owns an Arc<Job>.
#[derive(Debug)]
pub struct Owner(pub Arc<Job>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_stay_immutable_errors_are_bounded_and_closing_owner_cancels_worker() {
        let job = Arc::new(Job::default());
        let owner = Owner(job.clone());
        let first = job.snapshot();
        job.begin_file(100, "中文.txt", Some(1024));
        assert_eq!(first.phase, Phase::Scanning);
        assert_eq!(job.snapshot().phase, Phase::Copying);
        for i in 0..100 {
            job.completed_file(Some(&format!("{i}{}", "错".repeat(3000))));
        }
        let state = job.snapshot();
        assert_eq!(state.failed, 100);
        assert_eq!(state.errors.len(), 5);
        assert_eq!(state.other_errors, 95);
        assert!(state.errors.iter().all(|(e, _)| e.len() <= 4099));
        drop(owner);
        assert!(job.is_cancelled());
        job.finish();
        assert_eq!(job.snapshot().phase, Phase::Cancelled);
    }
}
