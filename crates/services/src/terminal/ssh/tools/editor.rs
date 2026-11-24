//! Session-scoped external editing cache; blocking disk work runs outside the actor.
use super::*;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    length: u64,
}
struct Edit {
    _directory: tempfile::TempDir,
    path: PathBuf,
    saved: Stamp,
    candidate: Option<Stamp>,
    error: String,
}
#[derive(Default)]
pub(crate) struct Editors {
    entries: tokio::sync::Mutex<HashMap<String, Edit>>,
    active: AtomicBool,
    busy: AtomicBool,
}
async fn stamp(path: PathBuf) -> Result<Stamp> {
    tokio::task::spawn_blocking(move || -> Result<_> {
        let meta = std::fs::metadata(path)?;
        Ok(Stamp {
            modified: meta.modified()?,
            length: meta.len(),
        })
    })
    .await?
}
impl Editors {
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
    pub async fn open(
        &self,
        client: &russh_sftp::client::SftpSession,
        remote: String,
        launch: bool,
    ) -> Result<Output> {
        let mut entries = self.entries.lock().await;
        if !entries.contains_key(&remote) {
            ensure!(entries.len() < 16, "同一会话最多同时监控 16 个外部编辑文件");
            let name = remote.rsplit('/').next().context("远端文件名为空")?;
            ensure!(
                crate::terminal::ssh::zmodem::safe_name(name),
                "文件名不适用于本地编辑路径"
            );
            let name = name.to_string();
            let directory = tokio::task::spawn_blocking(tempfile::tempdir).await??;
            let path = directory.path().join(name);
            sftp::execute(
                client,
                Operation::Download {
                    remote: remote.clone(),
                    local: path.clone(),
                },
            )
            .await?;
            let saved = stamp(path.clone()).await?;
            entries.insert(
                remote.clone(),
                Edit {
                    _directory: directory,
                    path,
                    saved,
                    candidate: None,
                    error: String::new(),
                },
            );
            self.active.store(true, Ordering::Relaxed);
        }
        let local = entries.get(&remote).context("编辑缓存缺失")?.path.clone();
        if launch {
            let path = local.clone();
            tokio::task::spawn_blocking(move || open::that_detached(path)).await??;
        }
        Ok(Output::EditCached { remote, local })
    }
    pub async fn synchronize(
        &self,
        client: &russh_sftp::client::SftpSession,
    ) -> Result<Option<String>> {
        if self.busy.swap(true, Ordering::Acquire) {
            return Ok(None);
        }
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.busy);
        let mut messages = Vec::new();
        let mut entries = self.entries.lock().await;
        for (remote, edit) in entries.iter_mut() {
            let result = async {
                let current = stamp(edit.path.clone()).await?;
                if current == edit.saved {
                    edit.candidate = None;
                    return Ok::<_, anyhow::Error>(false);
                }
                if edit.candidate != Some(current) {
                    edit.candidate = Some(current);
                    return Ok(false);
                }
                sftp::execute(
                    client,
                    Operation::Upload {
                        local: edit.path.clone(),
                        remote: remote.clone(),
                    },
                )
                .await?;
                edit.saved = current;
                edit.candidate = None;
                Ok(true)
            }
            .await;
            match result {
                Ok(true) => {
                    messages.push(format!("已自动回传 {remote}"));
                    edit.error.clear();
                }
                Ok(false) => {}
                Err(error) => {
                    let text = format!("自动回传 {remote} 失败：{error:#}");
                    if edit.error != text {
                        messages.push(text.clone());
                        edit.error = text;
                    }
                }
            }
        }
        Ok((!messages.is_empty()).then(|| messages.join("\n")))
    }
}
