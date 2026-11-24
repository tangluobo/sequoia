use super::*;
use std::{collections::HashMap, time::SystemTime};
struct Edit {
    local: PathBuf,
    saved: Option<(SystemTime, u64)>,
    observed: Option<(SystemTime, u64)>,
}
#[derive(Default)]
pub(crate) struct Editors {
    directory: Option<tempfile::TempDir>,
    files: HashMap<String, Edit>,
}
impl Editors {
    pub async fn open(
        &mut self,
        storage: &Storage,
        remote: String,
        launch: bool,
    ) -> Result<Output> {
        let local = if let Some(edit) = self.files.get(&remote) {
            edit.local.clone()
        } else {
            ensure!(
                self.files.len() < 16,
                "外部编辑缓存最多 16 个文件，请先关闭此连接"
            );
            if self.directory.is_none() {
                self.directory = Some(tokio::task::spawn_blocking(tempfile::tempdir).await??);
            }
            let name = remote.rsplit('/').next().context("缺少文件名")?.to_owned();
            ensure!(
                crate::terminal::ssh::zmodem::safe_name(&name),
                "无效的本地文件名"
            );
            let directory = self
                .directory
                .as_ref()
                .context("缓存目录未创建")?
                .path()
                .join(self.files.len().to_string());
            let local = directory.join(name);
            storage.download_file(&remote, local.clone()).await?;
            let signature = signature(&local).await;
            self.files.insert(
                remote.clone(),
                Edit {
                    local: local.clone(),
                    saved: signature,
                    observed: signature,
                },
            );
            local
        };
        if launch {
            let file = local.clone();
            tokio::task::spawn_blocking(move || open::that_detached(file)).await??;
        }
        Ok(Output::EditCached { remote, local })
    }
    pub async fn synchronize(&mut self, storage: &Storage) -> Result<Option<String>> {
        let mut count = 0;
        for (remote, edit) in &mut self.files {
            let current = signature(&edit.local).await;
            if current.is_some() && current != edit.saved && current == edit.observed {
                storage.upload_file(edit.local.clone(), remote).await?;
                edit.saved = current;
                count += 1;
            }
            edit.observed = current;
        }
        Ok((count > 0).then(|| format!("已回传 {count} 个外部编辑文件")))
    }
}
async fn signature(path: &Path) -> Option<(SystemTime, u64)> {
    let m = tokio::fs::metadata(path).await.ok()?;
    Some((m.modified().ok()?, m.len()))
}
