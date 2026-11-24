//! Enumerate directory metadata only after the Shell accepts the dropped roots.
use super::*;
use crate::files::operations::Entry;

impl Request {
    #[cfg(any(windows, test))]
    pub(super) fn wait_manifest(&self, cancelled: impl Fn() -> bool) -> Result<()> {
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
            if let Some(error) = self.failure() {
                anyhow::bail!(error.to_owned());
            }
            if self.manifest.get().is_some() {
                return Ok(());
            }
            state = self
                .stage
                .changed
                .wait_timeout(state, std::time::Duration::from_millis(50))
                .map_err(|_| anyhow::anyhow!("文件拖放等待失败"))?
                .0;
        }
    }
    pub async fn prepare<F, Fut>(&self, mut list: F) -> Result<()>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Entry>>>,
    {
        if !self.items.iter().any(|item| item.directory) || self.manifest.get().is_some() {
            return Ok(());
        }
        let result = async {
            let mut pending: Vec<_> = self
                .items
                .iter()
                .rev()
                .cloned()
                .map(|item| (item, 0))
                .collect();
            let mut manifest = Vec::new();
            let mut names = std::collections::HashSet::new();
            let mut retained = pending.iter().fold(0usize, |total, (item, _)| {
                total.saturating_add(item.path.capacity() + item.relative.capacity())
            });
            ensure!(retained <= 8 * 1024 * 1024, "拖出目录元数据超过 8 MiB");
            while let Some((item, depth)) = pending.pop() {
                ensure!(
                    depth <= 64 && manifest.len() + pending.len() < 4096,
                    "拖出目录超过 4096 项或 64 层"
                );
                ensure!(
                    names.insert(item.relative.to_lowercase()),
                    "拖出目录存在 Windows 同名路径：{}",
                    item.relative
                );
                ensure!(
                    item.relative.encode_utf16().count() < 260,
                    "拖出目录路径超过 Windows 长度限制"
                );
                if item.directory {
                    let children = list(item.path.clone()).await?;
                    ensure!(
                        manifest.len() + pending.len() + children.len() < 4096,
                        "拖出目录超过 4096 项"
                    );
                    for child in children.into_iter().rev() {
                        ensure!(
                            crate::terminal::ssh::zmodem::safe_name(&child.name),
                            "远端文件名不能用于系统拖放"
                        );
                        ensure!(!child.symlink, "目录拖出不跟随符号链接，请选择实际文件");
                        let child = Item {
                            path: format!("{}/{}", item.path.trim_end_matches('/'), child.name),
                            relative: format!("{}/{}", item.relative, child.name),
                            size: Some(child.size),
                            directory: child.directory,
                        };
                        retained = retained
                            .saturating_add(child.path.capacity() + child.relative.capacity());
                        ensure!(retained <= 8 * 1024 * 1024, "拖出目录元数据超过 8 MiB");
                        pending.push((child, depth + 1));
                    }
                }
                manifest.push(item);
            }
            let count = manifest.len();
            ensure!(
                self.manifest.set(manifest.into()).is_ok(),
                "拖出目录重复准备"
            );
            *self
                .stage
                .result
                .lock()
                .map_err(|_| anyhow::anyhow!("文件拖放状态锁失效"))? = vec![None; count];
            self.stage.changed.notify_all();
            Ok(())
        };
        let result = tokio::select! { biased; _ = self.cancel.cancelled() => Err(anyhow::anyhow!("文件拖放已取消")), result = result => result };
        if let Err(error) = &result {
            let _ = self.stage.failure.set(format!("{error:#}"));
            self.stage.changed.notify_all();
        }
        result
    }
    pub async fn directory_asset(&self, index: usize, remote: &str) -> Result<Arc<Asset>> {
        let asset = Asset::prepare(remote).await?;
        tokio::fs::create_dir(asset.path()).await?;
        self.publish(index, asset.clone());
        Ok(asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{
        operations::{Operation, Output},
        storage::Storage,
    };
    #[tokio::test]
    async fn cancellation_releases_the_native_directory_manifest_waiter() -> Result<()> {
        let request = Request::selection(vec![Item::new("/directory".into(), None, true)])?;
        let waiting = request.clone();
        let consumer = tokio::task::spawn_blocking(move || waiting.wait_manifest(|| false));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !request.stage.released.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        request.cancel.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), consumer)
                .await??
                .is_err()
        );
        assert!(request.manifest.get().is_none());
        Ok(())
    }
    #[tokio::test]
    async fn folder_export_starts_without_listing_and_enumerates_current_tree_after_drop()
    -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir_all(root.path().join("目录/空目录"))?;
        std::fs::write(root.path().join("目录/中文.txt"), b"before")?;
        std::fs::write(root.path().join("sibling.txt"), b"")?;
        let mut connection = tomato_core::Connection::new("LOCAL_DIRECTORY");
        connection.set("localDirectoryPath", root.path().to_string_lossy().as_ref());
        let storage = Storage::connect(&connection).await?;
        let path = |name: &str| root.path().join(name).to_string_lossy().replace('\\', "/");
        let request = Request::selection(vec![
            Item::new(path("目录"), None, true),
            Item::new(path("sibling.txt"), Some(0), false),
        ])?;
        let operation = storage.execute(Operation::ExportFile(request.clone()));
        tokio::pin!(operation);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    _ = operation.as_mut() => panic!("Export completed before drop"),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(2)) => {
                        if request.started() { break; }
                    }
                }
            }
        })
        .await?;
        assert!(request.started());
        assert_eq!(
            request.items().len(),
            2,
            "hover keeps only the selected roots"
        );
        assert!(
            request.manifest.get().is_none(),
            "no directory listing before release"
        );
        assert!(
            request
                .stage
                .result
                .lock()
                .unwrap()
                .iter()
                .all(Option::is_none),
            "no staged contents before drop acceptance"
        );
        std::fs::write(
            root.path().join("目录/拖动中新增.txt"),
            b"new while hovering",
        )?;
        std::fs::write(root.path().join("目录/中文.txt"), b"written while hovering")?;
        request.accept_drop();
        assert!(matches!(operation.await?, Output::ExportReady(..)));
        assert_eq!(request.items().len(), 5);
        assert!(
            request
                .items()
                .iter()
                .any(|item| item.relative == "目录/空目录" && item.directory)
        );
        for (index, item) in request.items().iter().enumerate() {
            let asset = request.wait_file(index, || false)?;
            if item.directory {
                assert!(asset.path().is_dir());
            } else if item.relative.ends_with("中文.txt") {
                assert_eq!(std::fs::read(asset.path())?, b"written while hovering");
            } else if item.relative.ends_with("拖动中新增.txt") {
                assert_eq!(std::fs::read(asset.path())?, b"new while hovering");
            } else {
                assert!(std::fs::read(asset.path())?.is_empty());
            }
        }
        Ok(())
    }
    #[tokio::test]
    async fn manifest_rejects_windows_collisions_and_links_without_reading_bytes() -> Result<()> {
        for symlink in [false, true] {
            let request = Request::selection(vec![Item::new("/root".into(), None, true)])?;
            let result = request
                .prepare(|_| async {
                    Ok(["Name.txt", "name.txt"]
                        .into_iter()
                        .map(|name| Entry {
                            name: name.into(),
                            path: format!("/root/{name}"),
                            directory: false,
                            symlink,
                            size: 1,
                            modified: 0,
                            permissions: 0,
                        })
                        .collect())
                })
                .await;
            assert!(result.is_err());
            assert!(request.failure().is_some());
        }
        Ok(())
    }
}
