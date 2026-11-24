//! One device transport/decoder; views retain independent cursors into shared pictures.
mod input;
use super::*;
pub(super) use input::{Event, Inputs};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;

type Key = (String, tokio::runtime::Id);
type Sessions = Mutex<HashMap<Key, Weak<Backend>>>;
static SESSIONS: OnceLock<Sessions> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct Pictures {
    revision: u64,
    published_at: Option<Instant>,
    rgb: Option<Arc<Frame>>,
    rgba: Option<Arc<RgbaFrame>>,
    yuv: Option<Arc<YuvFrame>>,
}
type Listener = Weak<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
struct Subscriber {
    notifier: Listener,
    yuv: bool,
}
#[derive(Default)]
struct Views {
    subscribers: BTreeMap<u64, Subscriber>,
    pictures: Pictures,
}
struct Backend {
    shared: Arc<Mutex<Shared>>,
    views: Arc<Mutex<Views>>,
    changed: Arc<tokio::sync::Notify>,
    input: mpsc::Sender<Event>,
    configuration: watch::Sender<Connection>,
    _worker: vnc::Worker,
}
pub(in crate::remote_desktop) struct Lease {
    backend: Arc<Backend>,
    id: u64,
    seen: Mutex<Seen>,
}
#[derive(Default)]
struct Seen {
    revisions: [u64; 3],
    requested: [Option<Instant>; 3],
}
pub(in crate::remote_desktop) fn acquire(
    connection: &Connection,
    runtime: &tokio::runtime::Handle,
    notifier: &Notifier,
) -> Result<Lease> {
    let endpoint = endpoint(connection)?;
    let key = (endpoint.to_lowercase(), runtime.id());
    let mut sessions = SESSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("ADB 会话池锁失效"))?;
    sessions.retain(|_, value| value.strong_count() > 0);
    let backend = if let Some(backend) = sessions.get(&key).and_then(Weak::upgrade) {
        backend
    } else {
        ensure!(sessions.len() < 64, "最多同时连接 64 个 ADB 设备");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let views = Arc::new(Mutex::new(Views::default()));
        let changed = Arc::new(tokio::sync::Notify::new());
        let (input, receiver) = mpsc::channel(64);
        let (configuration, updates) = watch::channel(connection.clone());
        let (state, listeners) = (shared.clone(), views.clone());
        let repaint: Notifier = Arc::new(Mutex::new(Some(Arc::new(move || {
            let pictures = state.lock().ok().map(|mut state| {
                (
                    state.frame.take(),
                    state.rgba_frame.take(),
                    state.yuv_frame.take(),
                )
            });
            let callbacks = listeners
                .lock()
                .ok()
                .map(|mut views| {
                    if let Some((rgb, rgba, yuv)) = pictures
                        && (rgb.is_some() || rgba.is_some() || yuv.is_some())
                    {
                        views.pictures = Pictures {
                            revision: views.pictures.revision.wrapping_add(1).max(1),
                            published_at: Some(Instant::now()),
                            rgb: rgb.map(Arc::new),
                            rgba: rgba.map(Arc::new),
                            yuv: yuv.map(Arc::new),
                        };
                    }
                    views
                        .subscribers
                        .values()
                        .filter_map(|s| s.notifier.upgrade())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for callback in callbacks {
                wake(&callback);
            }
        }))));
        let inputs = Inputs::new(receiver, views.clone(), changed.clone());
        let (state, listeners) = (shared.clone(), views.clone());
        let worker = vnc::Worker(runtime.spawn(async move {
            run(updates, inputs, &state, &listeners, &repaint).await;
        }));
        let backend = Arc::new(Backend {
            shared,
            views,
            changed,
            input,
            configuration,
            _worker: worker,
        });
        sessions.insert(key, Arc::downgrade(&backend));
        backend
    };
    drop(sessions);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    {
        let mut views = backend
            .views
            .lock()
            .map_err(|_| anyhow::anyhow!("ADB 窗口锁失效"))?;
        ensure!(
            views.subscribers.len() < 64,
            "同一 ADB 设备最多打开 64 个视图"
        );
        views.subscribers.insert(
            id,
            Subscriber {
                notifier: Arc::downgrade(notifier),
                yuv: false,
            },
        );
    }
    let lease = Lease {
        backend,
        id,
        seen: Default::default(),
    };
    lease.output(false);
    Ok(lease)
}
async fn run(
    mut updates: watch::Receiver<Connection>,
    mut inputs: Inputs,
    shared: &Arc<Mutex<Shared>>,
    views: &Arc<Mutex<Views>>,
    notifier: &Notifier,
) {
    loop {
        let connection = updates.borrow_and_update().clone();
        inputs.clear();
        if let Ok(mut state) = shared.lock() {
            state.status = Status::Connecting;
            state.adb_generation = state.adb_generation.wrapping_add(1);
            state.frame = None;
            state.rgba_frame = None;
            state.yuv_frame = None;
        }
        if let Ok(mut views) = views.lock() {
            let revision = views.pictures.revision.wrapping_add(1).max(1);
            views.pictures = Pictures {
                revision,
                ..Default::default()
            };
        }
        wake(notifier);
        let result = tokio::select! {
            result = super::run(connection, &mut inputs, shared, notifier) => result,
            changed = updates.changed() => { if changed.is_err() { return; } continue; }
        };
        if let Ok(mut state) = shared.lock() {
            state.status = Status::Failed(format!(
                "连接中断，正在重试：{}",
                result
                    .err()
                    .map_or_else(|| "ADB 画面通道已关闭".into(), |error| format!("{error:#}"))
            ));
        }
        wake(notifier);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            changed = updates.changed() => if changed.is_err() { return; },
        }
    }
}
impl Lease {
    pub(in crate::remote_desktop) fn shared(&self) -> Arc<Mutex<Shared>> {
        self.backend.shared.clone()
    }
    pub(in crate::remote_desktop) fn send(&self, input: Input) -> Result<()> {
        self.backend
            .input
            .try_send(Event {
                owner: self.id,
                input: Some(input),
            })
            .map_err(|_| anyhow::anyhow!("ADB 输入队列已满或会话已关闭"))
    }
    pub(in crate::remote_desktop) fn configure(&self, connection: &Connection) {
        let mut configured = self.backend.configuration.borrow().clone();
        for key in [
            "adbDisplayMode",
            "adbVideoQuality",
            "adbVideoMaxSize",
            "adbVideoBitRate",
        ] {
            if let Some(value) = connection.options.get(key) {
                configured.set(key, value.clone());
            } else {
                configured.options.remove(key);
            }
        }
        if let Ok(mut state) = self.backend.shared.lock() {
            state.status = Status::Connecting;
            state.adb_generation = state.adb_generation.wrapping_add(1);
            state.adb_video = None;
            state.frame = None;
            state.rgba_frame = None;
            state.yuv_frame = None;
        }
        if let Ok(mut views) = self.backend.views.lock() {
            let revision = views.pictures.revision.wrapping_add(1).max(1);
            views.pictures = Pictures {
                revision,
                ..Default::default()
            };
        }
        self.backend.configuration.send_replace(configured);
    }
    pub(in crate::remote_desktop) fn settings(&self, connection: &mut Connection) {
        let current = self.backend.configuration.borrow();
        for key in [
            "adbDisplayMode",
            "adbVideoQuality",
            "adbVideoMaxSize",
            "adbVideoBitRate",
        ] {
            if let Some(value) = current.options.get(key) {
                connection.set(key, value.clone());
            } else {
                connection.options.remove(key);
            }
        }
    }
    pub(in crate::remote_desktop) fn output(&self, yuv: bool) {
        if let Ok(mut views) = self.backend.views.lock()
            && let Some(subscriber) = views.subscribers.get_mut(&self.id)
        {
            subscriber.yuv = yuv;
        }
        self.sync_output();
    }
    fn sync_output(&self) {
        if let Ok(mut shared) = self.backend.shared.lock()
            && let Ok(views) = self.backend.views.lock()
        {
            shared.adb_yuv_output = views.subscribers.values().any(|s| s.yuv);
            shared.adb_rgba_output = views.subscribers.values().any(|s| !s.yuv);
        }
    }
    fn picture<T>(
        &self,
        index: usize,
        select: impl FnOnce(&mut Pictures, bool) -> Option<Arc<T>>,
    ) -> Option<Arc<T>> {
        let now = Instant::now();
        if let Ok(mut shared) = self.backend.shared.lock() {
            shared.adb_frame_requested_at = Some(now);
            shared.adb_frame_request_pending = true;
        }
        let mut views = self.backend.views.lock().ok()?;
        let mut seen = self.seen.lock().ok()?;
        let inactive = seen.requested[index]
            .is_some_and(|last| now.duration_since(last) > Duration::from_millis(250));
        let stale = inactive
            && views
                .pictures
                .published_at
                .is_none_or(|last| now.duration_since(last) > Duration::from_millis(250));
        seen.requested[index] = Some(now);
        if seen.revisions[index] == views.pictures.revision || stale {
            seen.revisions[index] = views.pictures.revision;
            return None;
        }
        seen.revisions[index] = views.pictures.revision;
        let single = views.subscribers.len() == 1;
        select(&mut views.pictures, single)
    }
    pub(in crate::remote_desktop) fn frame(&self) -> Option<Frame> {
        self.picture(
            0,
            |p, single| if single { p.rgb.take() } else { p.rgb.clone() },
        )
        .map(|p| Arc::try_unwrap(p).unwrap_or_else(|p| (*p).clone()))
    }
    pub(in crate::remote_desktop) fn rgba(&self) -> Option<RgbaFrame> {
        self.picture(1, |p, single| {
            if single {
                p.rgba.take()
            } else {
                p.rgba.clone()
            }
        })
        .map(|p| Arc::try_unwrap(p).unwrap_or_else(|p| (*p).clone()))
    }
    pub(in crate::remote_desktop) fn yuv(&self) -> Option<Arc<YuvFrame>> {
        self.picture(2, |p, _| p.yuv.clone())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut views) = self.backend.views.lock() {
            views.subscribers.remove(&self.id);
        }
        self.sync_output();
        self.backend.changed.notify_one();
        // The final view drops the only strong backend reference, aborting capture.
        // No idle encoder, per-view decoder or permanent cleanup task is retained.
    }
}
