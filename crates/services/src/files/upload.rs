//! Shared bounded upload planning and cancellation for storage and SSH adapters.
use super::object_copy::{Job, Snapshot};
use anyhow::{Context, Result, ensure};
use std::{path::PathBuf, sync::Arc};

type Stage = dyn Fn() -> Result<(Vec<PathBuf>, Box<dyn Send>), String> + Send + Sync;

#[derive(Clone)]
pub struct Deferred {
    pub(crate) retained_bytes: usize,
    stage: Arc<Stage>,
}

impl Deferred {
    pub fn new(retained_bytes: usize, stage: Arc<Stage>) -> Self {
        Self {
            retained_bytes,
            stage,
        }
    }
}

impl std::fmt::Debug for Deferred {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Deferred")
            .field("retained_bytes", &self.retained_bytes)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Entry {
    pub local: PathBuf,
    pub relative: String,
    pub directory: bool,
    pub empty: bool,
    pub bytes: u64,
    error: Option<String>,
}
pub(crate) trait Target: Sync {
    fn validate(&self, directory: &str) -> Result<()> {
        ensure!(
            !directory.is_empty() && !directory.contains('\0'),
            "上传目标目录无效"
        );
        Ok(())
    }
    fn object_storage(&self) -> bool;
    fn mkdir(&self, remote: &str) -> impl std::future::Future<Output = Result<()>> + Send;
    fn write(
        &self,
        entry: &Entry,
        remote: &str,
        job: &Arc<Job>,
    ) -> impl std::future::Future<Output = Result<u64>> + Send;
}

struct Plan {
    entries: Vec<Entry>,
    bytes: u64,
    retained: usize,
}
impl Plan {
    fn reserve(&mut self, local: &std::path::Path, relative: &str) -> Result<()> {
        self.retained = self
            .retained
            .saturating_add(2 * std::mem::size_of::<Entry>())
            .saturating_add(local.as_os_str().len().saturating_mul(2))
            .saturating_add(relative.len());
        ensure!(self.retained <= 32 * 1024 * 1024, "上传清单超过 32 MiB");
        Ok(())
    }
}

fn scan(roots: Vec<PathBuf>, job: &Job) -> Result<Plan> {
    ensure!(
        !roots.is_empty() && roots.len() <= 1000,
        "一次上传需要 1 到 1000 个文件或目录"
    );
    let mut plan = Plan {
        entries: Vec::new(),
        bytes: 0,
        retained: 0,
    };
    let mut pending = Vec::new();
    for root in roots.into_iter().rev() {
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .context("上传文件名无法读取")?
            .to_owned();
        ensure!(
            !matches!(name.as_str(), "." | "..") && !name.contains(['/', '\\', '\0']),
            "上传文件名无效"
        );
        plan.reserve(&root, &name)?;
        pending.push((root, name, 0usize));
    }
    while let Some((local, relative, depth)) = pending.pop() {
        if job.is_cancelled() {
            break;
        }
        ensure!(
            depth <= 64 && plan.entries.len() < 100_000,
            "上传目录超过深度或条目限制"
        );
        let metadata = std::fs::symlink_metadata(&local);
        let mut entry = Entry {
            local,
            relative,
            directory: false,
            empty: false,
            bytes: 0,
            error: None,
        };
        match metadata {
            Ok(metadata) if metadata.is_symlink() => {
                entry.error = Some("递归上传不跟随符号链接".into())
            }
            Ok(metadata) if metadata.is_dir() => {
                entry.directory = true;
                match std::fs::read_dir(&entry.local) {
                    Ok(children) => {
                        let mut found = Vec::new();
                        for child in children {
                            if job.is_cancelled() {
                                break;
                            }
                            let child = child?;
                            let name = child
                                .file_name()
                                .into_string()
                                .map_err(|_| anyhow::anyhow!("上传文件名不是 UTF-8"))?;
                            let path = child.path();
                            let relative = format!("{}/{name}", entry.relative);
                            ensure!(
                                found.len() < 20_000 && pending.len() + found.len() < 100_000,
                                "上传目录超过条目限制"
                            );
                            plan.reserve(&path, &relative)?;
                            found.push((path, relative, depth + 1));
                        }
                        entry.empty = found.is_empty();
                        pending.extend(found.into_iter().rev());
                    }
                    Err(error) => entry.error = Some(error.to_string()),
                }
            }
            Ok(metadata) if metadata.is_file() => {
                entry.bytes = metadata.len();
                plan.bytes = plan.bytes.saturating_add(entry.bytes);
            }
            Ok(_) => entry.error = Some("仅支持普通文件和目录".into()),
            Err(error) => entry.error = Some(error.to_string()),
        }
        job.found(plan.entries.len() + 1, &entry.relative);
        plan.entries.push(entry);
    }
    Ok(plan)
}

pub(crate) async fn run(
    target: &impl Target,
    directory: &str,
    mut paths: Vec<PathBuf>,
    deferred: Vec<Deferred>,
    job: Arc<Job>,
) -> Arc<Snapshot> {
    if let Err(error) = target.validate(directory) {
        job.fail(&error.to_string());
        return job.snapshot();
    }
    static LIMIT: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let limiter = LIMIT
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone();
    job.waiting();
    let permit = tokio::select! {
        biased;
        _=job.cancelled()=>{job.finish();return job.snapshot();},
        permit=limiter.acquire_owned()=>match permit {
            Ok(permit)=>permit,
            Err(error)=>{job.fail(&error.to_string());return job.snapshot();}
        }
    };
    job.scanning();
    let worker = job.clone();
    // A blocked filesystem scan keeps its permit even if its caller is cancelled.
    let mut scan_task = tokio::task::spawn_blocking(move || {
        let mut leases = Vec::with_capacity(deferred.len());
        let plan = (|| {
            for drop in deferred {
                if worker.is_cancelled() {
                    break;
                }
                let (mut staged, lease) = (drop.stage)()
                    .map_err(anyhow::Error::msg)
                    .context("读取压缩包拖入的文件失败")?;
                paths.append(&mut staged);
                leases.push(lease);
            }
            scan(paths, &worker)
        })();
        (plan, permit, leases)
    });
    let (plan, _permit, _leases) = tokio::select! {
        biased;
        _=job.cancelled()=>{scan_task.abort();job.finish();return job.snapshot();},
        result=&mut scan_task=>match result {
            Ok(result)=>result,
            Err(error)=>{job.fail(&error.to_string());return job.snapshot();}
        }
    };
    if job.is_cancelled() {
        job.finish();
        return job.snapshot();
    }
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            job.fail(&format!("{error:#}"));
            return job.snapshot();
        }
    };
    job.inventory(plan.entries.len(), plan.bytes);
    for entry in &plan.entries {
        if job.is_cancelled() {
            break;
        }
        job.begin_file(plan.entries.len(), &entry.relative, Some(entry.bytes));
        let remote = format!("{}/{}", directory.trim_end_matches('/'), entry.relative);
        let result = if let Some(error) = &entry.error {
            Err(anyhow::anyhow!("{error}"))
        } else {
            tokio::select! {
                biased;
                _ = job.cancelled() => break,
                result = async {
                    if entry.directory {
                        if !target.object_storage() || entry.empty { target.mkdir(&remote).await?; }
                    } else { target.write(entry, &remote, &job).await?; }
                    Ok::<_,anyhow::Error>(())
                } => result,
            }
        };
        job.account_bytes(entry.bytes);
        job.completed_file(
            result
                .as_ref()
                .err()
                .map(|error| error.to_string())
                .as_deref(),
        );
    }
    // Keep the task/permit alive until an interrupted multipart writer has aborted.
    job.wait_cleanup().await;
    job.finish();
    job.snapshot()
}
