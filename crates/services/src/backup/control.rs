//! Observer bridge for database actors: bounded logs and cancellation without polling the server.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Default, Debug)]
pub struct Progress {
    pub objects: u64,
    pub total: u64,
    pub records: u64,
    pub logs: VecDeque<String>,
    bytes: usize,
}
#[derive(Default)]
struct Shared {
    cancelled: AtomicBool,
    cancellation: tokio_util::sync::CancellationToken,
    progress: Mutex<Progress>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
}
#[derive(Clone, Default)]
pub struct Control(Arc<Shared>);
impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BackupControl")
    }
}
impl Control {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(Shared {
            notify: Some(Arc::new(notify)),
            ..Default::default()
        }))
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.cancellation.cancel();
        self.notify();
    }
    pub async fn wait_cancelled(&self) {
        self.0.cancellation.cancelled().await;
    }
    pub fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.cancelled(), "操作已取消");
        Ok(())
    }
    pub fn read<T>(&self, read: impl FnOnce(&Progress) -> T) -> Option<T> {
        self.0.progress.lock().ok().map(|progress| read(&progress))
    }
    fn update(&self, write: impl FnOnce(&mut Progress)) {
        if let Ok(mut progress) = self.0.progress.lock() {
            write(&mut progress);
        }
        self.notify();
    }
    fn notify(&self) {
        if let Some(notify) = &self.0.notify {
            notify();
        }
    }
    pub fn reset(&self, total: u64) {
        self.update(|progress| {
            progress.objects = 0;
            progress.records = 0;
            progress.total = total;
            progress.logs.clear();
            progress.bytes = 0;
        });
    }
    pub fn start(&self, total: u64) {
        self.update(|progress| progress.total = total);
    }
    pub fn object(&self) {
        self.update(|progress| progress.objects = progress.objects.saturating_add(1));
    }
    pub fn records(&self, count: u64) {
        self.update(|progress| progress.records = progress.records.saturating_add(count));
    }
    pub fn log(&self, text: &str) {
        let mut end = text.len().min(4096);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = text[..end].to_owned();
        self.update(|progress| {
            progress.bytes = progress.bytes.saturating_add(text.len());
            progress.logs.push_back(text);
            while progress.logs.len() > 256 || progress.bytes > 64 * 1024 {
                if let Some(old) = progress.logs.pop_front() {
                    progress.bytes = progress.bytes.saturating_sub(old.len());
                } else {
                    break;
                }
            }
        });
    }
}
