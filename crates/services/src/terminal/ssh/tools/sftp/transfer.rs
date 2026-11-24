use super::*;
use std::path::PathBuf;
pub(super) async fn upload(client: &SftpSession, local: PathBuf, remote: String) -> Result<u64> {
    let mut pending = vec![(local, remote, 0)];
    let mut visited = 0;
    let mut total = 0;
    while let Some((local, remote, depth)) = pending.pop() {
        visited += 1;
        ensure!(
            depth <= 64 && visited <= 100_000,
            "上传目录超过深度或条目限制"
        );
        let (local, meta) = tokio::task::spawn_blocking(move || {
            let meta = std::fs::symlink_metadata(&local)?;
            Ok::<_, std::io::Error>((local, meta))
        })
        .await??;
        ensure!(
            !meta.is_symlink(),
            "目录上传不跟随符号链接，请单独选择实际文件"
        );
        if meta.is_dir() {
            if let Ok(meta) = client.metadata(remote.clone()).await {
                ensure!(meta.is_dir(), "远端同名项不是目录");
            } else {
                client.create_dir(remote.clone()).await?;
            }
            let entries = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(local)? {
                    ensure!(entries.len() < 20_000, "目录超过 20,000 项");
                    entries.push(entry?.path());
                }
                Ok(entries)
            })
            .await??;
            for entry in entries {
                let name = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .context("本地文件名不是 UTF-8")?;
                ensure!(!name.contains('/'), "无效本地文件名");
                let target = format!("{}/{name}", remote.trim_end_matches('/'));
                pending.push((entry, target, depth + 1));
            }
        } else {
            ensure!(meta.is_file(), "仅支持上传普通文件和目录");
            total += upload_file(client, local, remote).await?;
        }
    }
    Ok(total)
}
pub(super) async fn download(client: &SftpSession, remote: String, local: PathBuf) -> Result<u64> {
    let mut pending = vec![(remote, local, 0)];
    let mut total = 0;
    let mut visited = 0;
    while let Some((remote, local, depth)) = pending.pop() {
        visited += 1;
        ensure!(
            depth <= 64 && visited <= 100_000,
            "下载目录超过深度或条目限制"
        );
        let meta = client.symlink_metadata(remote.clone()).await?;
        ensure!(!meta.is_symlink(), "目录下载不跟随远端符号链接");
        if meta.is_dir() {
            let directory = local.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                if let Ok(meta) = std::fs::symlink_metadata(&directory) {
                    ensure!(
                        meta.is_dir() && !meta.is_symlink(),
                        "本地同名目录是文件或符号链接"
                    );
                }
                std::fs::create_dir_all(directory)?;
                Ok(())
            })
            .await??;
            for (count, entry) in client.read_dir_limited(remote, 20_000).await?.enumerate() {
                ensure!(count < 20_000, "目录超过 20,000 项");
                let name = entry.file_name();
                ensure!(
                    crate::terminal::ssh::zmodem::safe_name(&name),
                    "远端文件名不适用于本地路径"
                );
                pending.push((entry.path(), local.join(name), depth + 1));
            }
        } else {
            total += download_file(client, remote, local).await?;
        }
    }
    Ok(total)
}
