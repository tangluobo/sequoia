//! Bounded download actor. UI lifetimes own the actor, sockets and task queue.
mod bt;
mod ed2k;
mod http;
mod source;
#[cfg(test)]
mod tests;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    pub url: String,
    pub protocol: String,
    pub directory: String,
    pub requested_threads: usize,
    pub file_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: i64,
    pub target_file: Option<String>,
    pub completed_file: Option<String>,
    pub state: String,
    pub status: String,
    pub actual_threads: usize,
    pub rebalance_count: u64,
    #[serde(rename = "rustId")]
    pub id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
impl Default for Record {
    fn default() -> Self {
        Self {
            url: String::new(),
            protocol: "HTTP".into(),
            directory: String::new(),
            requested_threads: 8,
            file_name: String::new(),
            downloaded_bytes: 0,
            total_bytes: -1,
            target_file: None,
            completed_file: None,
            state: "WAITING".into(),
            status: "等待中".into(),
            actual_threads: 0,
            rebalance_count: 0,
            id: uuid::Uuid::new_v4().to_string(),
            extra: Default::default(),
        }
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    tasks: Vec<Record>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}
pub enum Action {
    Configure(Settings),
    Add {
        url: String,
        directory: PathBuf,
        threads: usize,
    },
    Start(String),
    Cancel(String),
    Remove(String),
    ClearCompleted,
    Limit(usize),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub directory: String,
    pub threads: usize,
    pub concurrent: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            directory: std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
                .display()
                .to_string(),
            threads: 8,
            concurrent: 2,
        }
    }
}
pub struct Snapshot {
    pub rows: Vec<Record>,
    pub error: Option<String>,
    pub settings: Settings,
}
struct Shared {
    rows: Vec<Arc<Mutex<Record>>>,
    error: Option<String>,
    settings: Settings,
}
type Wake = Arc<dyn Fn() + Send + Sync>;
pub struct Manager {
    commands: mpsc::Sender<Action>,
    shared: Arc<Mutex<Shared>>,
    revision: Arc<AtomicU64>,
}
struct Entry {
    record: Arc<Mutex<Record>>,
    job: Option<crate::backup::Control>,
    remove: bool,
}
impl Manager {
    pub fn open(path: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (commands, receiver) = mpsc::channel(32);
        let shared = Arc::new(Mutex::new(Shared {
            rows: Vec::new(),
            error: None,
            settings: Settings::default(),
        }));
        let revision = Arc::new(AtomicU64::new(0));
        let state = shared.clone();
        let changes = revision.clone();
        let wake: Wake = Arc::new(wake);
        std::thread::spawn(move || {
            let result = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .max_blocking_threads(4)
                    .thread_stack_size(512 * 1024)
                    .thread_keep_alive(std::time::Duration::from_secs(2))
                    .build()?;
                runtime.block_on(actor(path, receiver, &state, &changes, &wake))
            })();
            if let Err(error) = result
                && let Ok(mut shared) = state.lock()
            {
                shared.error = Some(error.to_string());
                changes.fetch_add(1, Ordering::Relaxed);
                wake();
            }
        });
        Self {
            commands,
            shared,
            revision,
        }
    }
    pub fn send(&self, action: Action) -> Result<()> {
        if let Action::Add { url, directory, .. } = &action {
            validate_source(url)?;
            ensure!(directory.as_os_str().len() <= 32768, "保存路径过长");
        }
        if let Action::Configure(settings) = &action {
            ensure!(settings.directory.len() <= 32768, "保存路径过长");
        }
        self.commands
            .try_send(action)
            .map_err(|_| anyhow::anyhow!("下载器忙或已关闭"))
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
    pub fn snapshot(&self) -> Snapshot {
        match self.shared.lock() {
            Ok(s) => Snapshot {
                settings: s.settings.clone(),
                rows: s
                    .rows
                    .iter()
                    .filter_map(|r| r.lock().ok().map(|r| r.clone()))
                    .collect(),
                error: s.error.clone(),
            },
            Err(_) => Snapshot {
                settings: Settings::default(),
                rows: Vec::new(),
                error: Some("下载器状态锁失效".into()),
            },
        }
    }
}
pub fn validate_source(value: &str) -> Result<()> {
    source::Source::parse(value)?;
    Ok(())
}
pub(crate) fn sanitize_name(value: &str) -> String {
    let value = value
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    let mut value = value.trim_end_matches(['.', ' ']).to_owned();
    if value.trim_matches(['.', ' ']).is_empty() {
        return "download".into();
    }
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        value.insert(0, '_');
    }
    value
}
fn changed(revision: &AtomicU64, wake: &Wake) {
    revision.fetch_add(1, Ordering::Relaxed);
    wake();
}
fn save(
    path: &std::path::Path,
    original: &mut Option<Vec<u8>>,
    saved_fingerprint: &mut [u8; 20],
    store: &mut Store,
    entries: &[Entry],
) -> Result<()> {
    store.version = 1;
    store.tasks = entries
        .iter()
        .map(|e| e.record.lock().unwrap().clone())
        .collect();
    let data = serde_json::to_vec_pretty(store)?;
    let fingerprint: [u8; 20] = Sha1::digest(&data).into();
    // Compare the decoded model, retaining only a small fingerprint. Opening and
    // closing an old file must not rewrite its whitespace or omitted defaults.
    if fingerprint == *saved_fingerprint {
        return Ok(());
    }
    let current = match std::fs::read(path) {
        Ok(v) => Some(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    ensure!(
        current == *original,
        "下载记录已由其他程序修改，请重新打开下载器后再保存"
    );
    if let Some(bytes) = &current {
        let backup = path.with_extension("json.rust-backup");
        if !backup.exists() {
            tomato_core::config::atomic_write(&backup, bytes)?;
        }
    }
    tomato_core::config::atomic_write(path, &data)?;
    *original = Some(data);
    *saved_fingerprint = fingerprint;
    Ok(())
}
async fn actor(
    path: PathBuf,
    mut commands: mpsc::Receiver<Action>,
    shared: &Arc<Mutex<Shared>>,
    revision: &Arc<AtomicU64>,
    wake: &Wake,
) -> Result<()> {
    crate::init_crypto();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("json.rust-lock"))?;
    // A view can reopen before its previous actor has finished dropping sockets
    // and the lease. Wait off the UI thread, retaining cross-process exclusion.
    let started = std::time::Instant::now();
    loop {
        let attempt = _lease.try_lock();
        if attempt.is_ok() {
            break;
        }
        if started.elapsed() >= std::time::Duration::from_secs(2) {
            attempt.context("下载记录已由另一个 Rust 下载器使用")?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut original = match std::fs::read(&path) {
        Ok(v) => {
            ensure!(v.len() <= 8 * 1024 * 1024, "下载记录超过 8 MiB");
            Some(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut store: Store = original
        .as_ref()
        .map(|v| serde_json::from_slice(v))
        .transpose()?
        .unwrap_or_default();
    ensure!(
        original.is_none() || store.version == 1,
        "无法识别下载记录版本"
    );
    ensure!(store.tasks.len() <= 1000, "最多加载 1000 条下载记录");
    let mut saved_fingerprint: [u8; 20] = Sha1::digest(serde_json::to_vec_pretty(&store)?).into();
    let mut entries: Vec<Entry> = store
        .tasks
        .drain(..)
        .map(|mut r| {
            if r.id.is_empty() {
                r.id = uuid::Uuid::new_v4().to_string();
            }
            if matches!(r.state.as_str(), "DOWNLOADING" | "WAITING") {
                r.state = "WAITING".into();
                r.status = if r.target_file.is_some() {
                    "等待续传"
                } else {
                    "等待中"
                }
                .into();
            } else if r.state == "CANCELLING" {
                r.state = "CANCELLED".into();
                r.status = "已取消".into();
            } else if r.state == "COMPLETED"
                && !r
                    .completed_file
                    .as_ref()
                    .is_some_and(|p| std::path::Path::new(p).is_file())
            {
                r.state = "FAILED".into();
                r.status = "已完成文件不存在".into();
                r.completed_file = None;
            }
            Entry {
                record: Arc::new(Mutex::new(r)),
                job: None,
                remove: false,
            }
        })
        .collect();
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let mut jobs = tokio::task::JoinSet::new();
    let mut checkpoint = tokio::time::interval(std::time::Duration::from_secs(2));
    checkpoint.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut settings: Settings = store
        .extra
        .get("rustDownloaderSettings")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    settings.threads = settings.threads.clamp(1, 32);
    settings.concurrent = settings.concurrent.clamp(1, 8);
    let mut limit = settings.concurrent.clamp(1, 8);
    shared.lock().unwrap().settings = settings;
    shared.lock().unwrap().rows = entries.iter().map(|e| e.record.clone()).collect();
    changed(revision, wake);
    loop {
        while jobs.len() < limit {
            let Some(entry) = entries
                .iter_mut()
                .find(|e| e.job.is_none() && e.record.lock().unwrap().state == "WAITING")
            else {
                break;
            };
            let control = crate::backup::Control::default();
            entry.job = Some(control.clone());
            let row = entry.record.clone();
            let id = row.lock().unwrap().id.clone();
            {
                let mut r = row.lock().unwrap();
                r.state = "DOWNLOADING".into();
                r.status = "连接中".into();
            }
            let (client, revision, wake) = (client.clone(), revision.clone(), wake.clone());
            let profile = path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_owned();
            jobs.spawn(async move {
                let source = source::Source::parse(&row.lock().unwrap().url);
                let result = match source {
                    Ok(source::Source::Ed2k(link)) => ed2k::run(link, &profile, &row, &control, &revision, &wake, &client).await,
                    Ok(source::Source::Bt(source)) => bt::run(source, &row, &control, &revision, &wake, &client).await,
                    Ok(source::Source::Http(_)) => tokio::select! { result=http::run(&client,&row,&control,&revision,&wake)=>result, _=control.wait_cancelled()=>Err(anyhow::anyhow!("已取消")) },
                    Err(error) => Err(error),
                };
                (id,result)
            });
        }
        let mut persist = true;
        tokio::select! {
            _ = checkpoint.tick(), if !jobs.is_empty() => {},
            command=commands.recv()=> {
                let Some(command)=command else {for entry in &entries {if let Some(c)=&entry.job {c.cancel();}}while jobs.join_next().await.is_some(){}break;};
                match command {
                    Action::Configure(mut settings)=> {
                        settings.threads=settings.threads.clamp(1,32);settings.concurrent=settings.concurrent.clamp(1,8);limit=settings.concurrent;
                        store.extra.insert("rustDownloaderSettings".into(),serde_json::to_value(&settings)?);
                        shared.lock().unwrap().settings=settings;
                    },
                    Action::Add{url,directory,threads}=> {
                        if entries.len()>=1000 {shared.lock().unwrap().error=Some("最多保留 1000 条下载记录".into());}
                        else {match source::Source::parse(&url) {
                            Ok(source)=> {
                                let record=source.record(url,directory,threads);
                                entries.push(Entry{record:Arc::new(Mutex::new(record)),job:None,remove:false});
                            },
                            Err(error)=>shared.lock().unwrap().error=Some(error.to_string()),
                        }}
                    },
                    Action::Limit(n)=>limit=n.clamp(1,8),
                    Action::ClearCompleted=>entries.retain(|e| e.job.is_some() || !matches!(e.record.lock().unwrap().state.as_str(),"COMPLETED"|"FAILED"|"CANCELLED")),
                    Action::Start(id)=> {
                        if let Some(entry)=entries.iter_mut().find(|e| e.record.lock().unwrap().id==id) && entry.job.is_none() {
                            let mut row=entry.record.lock().unwrap();
                            if row.state!="COMPLETED" {row.state="WAITING".into();row.status="等待中".into();}
                        }
                    },
                    Action::Cancel(id)=> {
                        if let Some(entry)=entries.iter_mut().find(|e| e.record.lock().unwrap().id==id) {
                            let mut row=entry.record.lock().unwrap();
                            if let Some(control)=&entry.job {control.cancel();row.state="CANCELLING".into();row.status="正在取消".into();}
                            else if row.state=="WAITING" {row.state="CANCELLED".into();row.status="已取消".into();}
                        }
                    },
                    Action::Remove(id)=> {
                        if let Some(entry)=entries.iter_mut().find(|e| e.record.lock().unwrap().id==id) && let Some(control)=&entry.job {control.cancel();entry.remove=true;}
                        entries.retain(|e| e.record.lock().unwrap().id!=id || e.job.is_some());
                    },
                }
            },
            result=jobs.join_next(),if !jobs.is_empty()=> {
                if let Some(Ok((id,result)))=result && let Some(entry)=entries.iter_mut().find(|e| e.record.lock().unwrap().id==id) {
                    let cancelled=entry.job.take().is_some_and(|c| c.cancelled());
                    let mut row=entry.record.lock().unwrap();
                    match result {Ok(path)=>{row.state="COMPLETED".into();row.status="已完成".into();row.completed_file=Some(path.display().to_string());},Err(error)=>{row.state=if cancelled {"CANCELLED"}else {"FAILED"}.into();row.status=if cancelled {"已取消".into()}else {error.to_string()};}}

                } else {persist=false;}
                entries.retain(|e| !e.remove || e.job.is_some());
            },
        }
        shared.lock().unwrap().rows = entries.iter().map(|e| e.record.clone()).collect();
        if persist
            && let Err(error) = save(
                &path,
                &mut original,
                &mut saved_fingerprint,
                &mut store,
                &entries,
            )
        {
            shared.lock().unwrap().error = Some(error.to_string());
        }
        changed(revision, wake);
    }
    for entry in &entries {
        let mut r = entry.record.lock().unwrap();
        if r.state == "DOWNLOADING" {
            r.state = "CANCELLED".into();
            r.status = "已取消".into();
        }
    }
    save(
        &path,
        &mut original,
        &mut saved_fingerprint,
        &mut store,
        &entries,
    )
    .context("保存下载记录")
}
