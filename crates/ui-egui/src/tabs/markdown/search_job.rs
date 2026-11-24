use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    runtime::Handle,
    sync::{Semaphore, oneshot},
};
use tomato_core::search::{MAX_REPLACEMENT, MAX_TEXT, Search};

pub(crate) type Key = (u64, String, bool, bool);

#[derive(Clone)]
struct Executor {
    runtime: Handle,
    slots: Arc<Semaphore>,
    snapshots: Arc<Semaphore>,
}

fn executor_id() -> egui::Id {
    egui::Id::new("tomato-markdown-search-executor")
}

pub(crate) fn install(ctx: &egui::Context, runtime: &Handle) {
    ctx.data_mut(|data| {
        data.insert_temp(
            executor_id(),
            Executor {
                runtime: runtime.clone(),
                slots: Arc::new(Semaphore::new(1)),
                snapshots: Arc::new(Semaphore::new(2)),
            },
        )
    });
}

pub(crate) enum Work {
    Find,
    Replace {
        search: Arc<Search>,
        index: Option<usize>,
        value: Arc<String>,
    },
}

pub(crate) enum Output {
    Matches(Arc<Search>),
    Replaced(String, Option<Range<usize>>),
}

pub(crate) enum Notice {
    Replaced(u64),
    Error(String),
}

pub(crate) struct Job {
    pub key: Key,
    replacement: Option<Arc<String>>,
    cancelled: Arc<AtomicBool>,
    result: oneshot::Receiver<Result<Output, String>>,
}

impl Job {
    pub(crate) fn start(
        ctx: &egui::Context,
        key: Key,
        text: &str,
        work: Work,
    ) -> Result<Option<Self>, String> {
        if text.len() > MAX_TEXT {
            return Err("查找正文超过 8 MiB".into());
        }
        let executor = ctx
            .data(|data| data.get_temp::<Executor>(executor_id()))
            .ok_or_else(|| "查找后台不可用".to_owned())?;
        let Ok(snapshot) = executor.snapshots.try_acquire_owned() else {
            return Ok(None);
        };
        let replacement = match &work {
            Work::Find => None,
            Work::Replace { value, .. } if value.len() <= MAX_REPLACEMENT => Some(value.clone()),
            Work::Replace { .. } => return Err("正则替换模板超过 256 KiB".into()),
        };
        let text = text.to_owned();
        let worker_key = key.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (send, result) = oneshot::channel();
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        executor.runtime.spawn(async move {
            let _snapshot = snapshot;
            let permit = executor.slots.acquire_owned().await;
            let outcome = match permit {
                Ok(permit) if !worker_cancelled.load(Ordering::Relaxed) => {
                    tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        let cancelled = || worker_cancelled.load(Ordering::Relaxed);
                        match work {
                            Work::Find => Search::scan(
                                &text,
                                &worker_key.1,
                                worker_key.2,
                                worker_key.3,
                                cancelled,
                            )
                            .map(|search| Output::Matches(Arc::new(search))),
                            Work::Replace {
                                search,
                                index,
                                value,
                            } => {
                                let hit = index.and_then(|index| search.hits.get(index).cloned());
                                search
                                    .replace_checked(&text, index, &value, MAX_TEXT, cancelled)
                                    .map(|text| Output::Replaced(text, hit))
                            }
                        }
                        .map_err(|error| error.to_string())
                    })
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()))
                }
                _ => Err("已取消查找或替换".into()),
            };
            let _ = send.send(outcome);
            ctx.request_repaint_of(viewport);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
        Ok(Some(Self {
            key,
            replacement,
            cancelled,
            result,
        }))
    }

    pub(crate) fn is_replacement(&self) -> bool {
        self.replacement.is_some()
    }

    pub(crate) fn applies(&self, key: &Key, replacement: &str) -> bool {
        !self.cancelled.load(Ordering::Relaxed)
            && self.key == *key
            && self
                .replacement
                .as_ref()
                .is_none_or(|value| value.as_str() == replacement)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn poll(&mut self) -> Option<Result<Output, String>> {
        use oneshot::error::TryRecvError;
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Closed) => Some(Err("查找后台已关闭".into())),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}
