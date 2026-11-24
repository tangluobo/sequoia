//! A disk-backed export lease. No whole-file buffers or external-editor watchers.
mod manifest;
#[cfg(windows)]
mod native;
mod streaming;
#[cfg(test)]
mod streaming_tests;
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
pub(crate) use streaming::Writer;
#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub cancel: tokio_util::sync::CancellationToken,
    items: Arc<[Item]>,
    manifest: Arc<OnceLock<Arc<[Item]>>>,
    deferred: bool,
    stage: Arc<Stage>,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub path: String,
    pub size: Option<u64>,
    pub directory: bool,
    relative: String,
}
impl Item {
    pub fn new(path: String, size: Option<u64>, directory: bool) -> Self {
        let relative = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        Self {
            path,
            size,
            directory,
            relative,
        }
    }
}
#[derive(Debug, Default)]
struct Stage {
    started: AtomicBool,
    result: Mutex<Vec<Option<AssetResult>>>,
    changed: Condvar,
    failure: OnceLock<String>,
    released: AtomicBool,
    release: tokio::sync::Notify,
}
type AssetResult = std::result::Result<Arc<Asset>, String>;
impl Request {
    #[cfg(windows)]
    fn ready(&self, index: usize) -> Option<Arc<Asset>> {
        self.stage
            .result
            .try_lock()
            .ok()?
            .get(index)?
            .as_ref()?
            .as_ref()
            .ok()
            .cloned()
    }
    pub fn started(&self) -> bool {
        self.stage.started.load(Ordering::Acquire)
    }
    pub fn same_job(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stage, &other.stage)
    }
    pub fn failure(&self) -> Option<&str> {
        self.stage.failure.get().map(String::as_str)
    }
    pub fn new(path: String) -> Self {
        let items = vec![Item::new(path.clone(), None, false)];
        Self::with_items(items, false)
    }
    fn with_items(items: Vec<Item>, deferred: bool) -> Self {
        let count = items.len();
        Self {
            path: items[0].path.clone(),
            cancel: Default::default(),
            items: items.into(),
            manifest: Default::default(),
            deferred,
            stage: Arc::new(Stage {
                result: Mutex::new(vec![None; count]),
                ..Default::default()
            }),
        }
    }
    pub fn selection(items: Vec<Item>) -> Result<Self> {
        ensure!(
            !items.is_empty() && items.len() <= 4096,
            "每次可拖出 1–4096 个文件"
        );
        let mut names = std::collections::HashSet::new();
        for item in &items {
            let name = item
                .path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default();
            ensure!(
                crate::terminal::ssh::zmodem::safe_name(name),
                "远端文件名不能用于系统拖放"
            );
            ensure!(
                names.insert(name.to_lowercase()),
                "所选文件存在 Windows 同名路径：{name}"
            );
        }
        Ok(Self::with_items(items, true))
    }
    pub fn items(&self) -> &[Item] {
        self.manifest.get().unwrap_or(&self.items)
    }
    pub fn queued_bytes(&self) -> usize {
        self.path.capacity().saturating_add(
            self.items()
                .iter()
                .map(|i| std::mem::size_of::<Item>() + i.path.capacity() + i.relative.capacity())
                .sum::<usize>(),
        )
    }
    /// Accept a completed drop. Merely inspecting metadata must never call this.
    pub fn accept_drop(&self) {
        self.stage.released.store(true, Ordering::Release);
        self.stage.release.notify_one();
    }
    pub fn publish(&self, index: usize, asset: Arc<Asset>) {
        if let Ok(mut state) = self.stage.result.lock() {
            if let Some(slot) = state.get_mut(index) {
                *slot = Some(Ok(asset));
            }
            self.stage.changed.notify_all();
        }
    }
    pub async fn run(
        &self,
        operation: impl std::future::Future<Output = Result<super::operations::Output>>,
    ) -> Result<super::operations::Output> {
        self.stage.started.store(true, Ordering::Release);
        if self.deferred && !self.stage.released.load(Ordering::Acquire) {
            tokio::select! { biased; _ = self.cancel.cancelled() => {}, _ = self.stage.release.notified() => {} }
        }
        let result = tokio::select! { biased; _ = self.cancel.cancelled() => Err(anyhow::anyhow!("文件拖放已取消")), result = operation => result };
        if let Err(error) = &result
            && !self.cancel.is_cancelled()
        {
            let _ = self.stage.failure.set(format!("{error:#}"));
        }
        if let Ok(mut state) = self.stage.result.lock() {
            if let Err(error) = &result {
                for asset in state
                    .iter()
                    .flatten()
                    .filter_map(|result| result.as_ref().ok())
                {
                    asset.stream.fail(format!("{error:#}"));
                }
            }
            let published = match &result {
                Ok(super::operations::Output::ExportReady(_, asset)) => Ok(asset.clone()),
                Ok(_) => Err("文件拖放返回了不匹配的结果".into()),
                Err(error) => Err(format!("{error:#}")),
            };
            for slot in state.iter_mut() {
                if slot.is_none() {
                    *slot = Some(published.clone());
                }
            }
            self.stage.changed.notify_all();
        }
        result
    }
    #[cfg(any(windows, test))]
    fn wait_file(&self, index: usize, cancelled: impl Fn() -> bool) -> Result<Arc<Asset>> {
        self.accept_drop();
        let mut state = self
            .stage
            .result
            .lock()
            .map_err(|_| anyhow::anyhow!("文件拖放状态锁失效"))?;
        loop {
            ensure!(
                !self.cancel.is_cancelled() && !cancelled(),
                "文件拖放已取消"
            );
            if let Some(Some(result)) = state.get(index) {
                return result.clone().map_err(anyhow::Error::msg);
            }
            state = self
                .stage
                .changed
                .wait_timeout(state, std::time::Duration::from_millis(50))
                .map_err(|_| anyhow::anyhow!("文件拖放等待失败"))?
                .0;
        }
    }
    #[cfg(test)]
    fn wait(&self, cancelled: impl Fn() -> bool) -> Result<Arc<Asset>> {
        self.wait_file(0, cancelled)
    }
}
pub struct Drag {
    #[cfg(windows)]
    native: tomato_native_drag::Drag,
}
impl Drag {
    pub fn diagnostics(&self) -> String {
        #[cfg(windows)]
        {
            format!("{:?}", self.native.diagnostics())
        }
        #[cfg(not(windows))]
        {
            "native drag is unavailable".into()
        }
    }
    pub fn try_result(&self) -> Option<Result<bool>> {
        #[cfg(windows)]
        {
            self.native.try_result()
        }
        #[cfg(not(windows))]
        {
            Some(Err(anyhow::anyhow!("当前平台不支持系统文件拖出")))
        }
    }
}
pub fn begin(request: Request) -> Result<Drag> {
    #[cfg(windows)]
    {
        Ok(Drag {
            native: native::begin(request)?,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        anyhow::bail!("当前平台不支持系统文件拖出，请使用下载菜单")
    }
}
#[derive(Debug)]
pub struct Asset {
    directory: tempfile::TempDir,
    path: PathBuf,
    stream: streaming::State,
}
impl Asset {
    pub async fn prepare(remote: &str) -> Result<Arc<Self>> {
        let name = remote
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .context("文件名为空")?
            .to_owned();
        ensure!(
            crate::terminal::ssh::zmodem::safe_name(&name),
            "远端文件名不能用于系统拖放"
        );
        tokio::task::spawn_blocking(move || {
            let directory = tempfile::Builder::new().prefix("tomato-drag-").tempdir()?;
            let path = directory.path().join(name);
            Ok(Arc::new(Self {
                directory,
                path,
                stream: Default::default(),
            }))
        })
        .await?
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}
#[cfg(windows)]
pub fn start(asset: Arc<Asset>) -> Result<()> {
    tomato_native_drag::start(asset.path(), asset.clone())
}
#[cfg(not(windows))]
pub fn start(_asset: Arc<Asset>) -> Result<()> {
    anyhow::bail!("当前平台不支持系统文件拖出，请使用下载菜单")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn selected_files_wait_for_drop_and_copy_changed_source_bytes_and_empty_files()
    -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut connection = tomato_core::Connection::new("LOCAL_DIRECTORY");
        connection.set(
            "localDirectoryPath",
            directory.path().to_string_lossy().as_ref(),
        );
        let mut items = Vec::new();
        for name in ["多选.txt", "second.txt", "empty.txt"] {
            let path = directory.path().join(name);
            tokio::fs::write(&path, b"").await?;
            items.push(Item::new(
                path.to_string_lossy().replace('\\', "/"),
                Some(0),
                false,
            ));
        }
        let request = Request::selection(items)?;
        let storage = super::super::storage::Storage::connect(&connection).await?;
        let (output, first) = {
            let execute = storage.execute(super::super::operations::Operation::ExportFile(
                request.clone(),
            ));
            tokio::pin!(execute);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(80), execute.as_mut())
                    .await
                    .is_err()
            );
            assert!(request.started());
            assert!(
                request
                    .stage
                    .result
                    .lock()
                    .unwrap()
                    .iter()
                    .all(Option::is_none)
            );
            tokio::fs::write(directory.path().join("多选.txt"), b"written while hovering").await?;
            let waiting = request.clone();
            let reader = tokio::task::spawn_blocking(move || waiting.wait_file(0, || false));
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(5), execute.as_mut()).await??;
            let first = reader.await??;
            (output, first)
        };
        assert_eq!(std::fs::read(first.path())?, b"written while hovering");
        assert_eq!(std::fs::read(request.wait_file(2, || false)?.path())?, b"");
        let staged: Vec<_> = request
            .stage
            .result
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.as_ref().unwrap().as_ref().unwrap().directory().to_owned())
            .collect();
        drop((output, first, request));
        assert!(staged.iter().all(|path| !path.exists()));
        Ok(())
    }
    #[tokio::test]
    async fn failed_file_download_reaches_the_native_waiter_and_survives_later_cancellation()
    -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut connection = tomato_core::Connection::new("LOCAL_DIRECTORY");
        connection.set(
            "localDirectoryPath",
            directory.path().to_string_lossy().as_ref(),
        );
        let storage = super::super::storage::Storage::connect(&connection).await?;
        let request = Request::new(
            directory
                .path()
                .join("missing.txt")
                .to_string_lossy()
                .replace('\\', "/"),
        );
        assert!(
            storage
                .execute(super::super::operations::Operation::ExportFile(
                    request.clone()
                ))
                .await
                .is_err()
        );
        let failure = request
            .failure()
            .expect("publish the protocol failure")
            .to_owned();
        assert!(!failure.is_empty());
        assert!(request.wait(|| false).is_err());
        request.cancel.cancel();
        assert_eq!(
            request.failure(),
            Some(failure.as_str()),
            "UI cleanup must preserve the actual error"
        );
        Ok(())
    }
    #[tokio::test]
    async fn delayed_export_is_available_before_ui_reply_and_cancel_releases_a_waiting_consumer()
    -> Result<()> {
        let request = Request::new("/delayed.txt".into());
        let waiting = request.clone();
        let consumer = tokio::task::spawn_blocking(move || waiting.wait(|| false));
        let asset = Asset::prepare(&request.path).await?;
        tokio::fs::write(asset.path(), b"delayed disk bytes").await?;
        let output = request
            .run(async {
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                Ok(super::super::operations::Output::ExportReady(
                    request.path.clone(),
                    asset.clone(),
                ))
            })
            .await?;
        let resolved =
            tokio::time::timeout(std::time::Duration::from_secs(2), consumer).await???;
        assert_eq!(std::fs::read(resolved.path())?, b"delayed disk bytes");
        drop((output, resolved, asset, request));
        let request = Request::new("/cancelled.txt".into());
        let waiting = request.clone();
        let consumer = tokio::task::spawn_blocking(move || waiting.wait(|| false));
        request.cancel.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), consumer)
                .await??
                .is_err()
        );
        Ok(())
    }
}
