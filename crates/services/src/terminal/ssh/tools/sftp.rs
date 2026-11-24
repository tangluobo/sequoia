mod transfer;
use super::{Entry, Operation, Output};
use anyhow::{Context, Result, ensure};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const CHUNK: usize = 64 * 1024;
const TEXT_LIMIT: usize = 8 * 1024 * 1024;

pub(super) async fn export_file(
    client: &SftpSession,
    request: &crate::files::export::Request,
    index: usize,
    remote: String,
) -> Result<std::sync::Arc<crate::files::export::Asset>> {
    let mut input = client.open(remote.clone()).await?;
    let mut output = crate::files::export::Writer::new(&remote).await?;
    request.publish(index, output.asset());
    let mut bytes = vec![0; CHUNK];
    loop {
        let count = input.read(&mut bytes).await?;
        if count == 0 {
            break;
        }
        output.write_all(&bytes[..count]).await?;
        output.flush().await?;
    }
    output.finish().await
}

async fn read_image(
    client: &SftpSession,
    request: crate::files::images::Request,
) -> Result<Output> {
    use crate::files::images;
    let cancel = request.cancel.clone();
    let read = async {
        let permit = request.permit().await?;
        let meta = client.metadata(request.path.clone()).await?;
        ensure!(
            meta.size.unwrap_or(0) <= images::MAX_BYTES as u64,
            "图片超过 8 MiB"
        );
        let mut file = client.open(request.path.clone()).await?;
        let mut bytes =
            Vec::with_capacity(meta.size.unwrap_or(0).min(images::MAX_BYTES as u64) as usize);
        let mut chunk = vec![0; CHUNK];
        loop {
            let count = file.read(&mut chunk).await?;
            if count == 0 {
                break;
            }
            images::append(&mut bytes, &chunk[..count])?;
        }
        let path = request.path.clone();
        let mode = request.mode;
        let asset = images::decode(request, bytes, permit).await?;
        Ok(Output::Image { path, mode, asset })
    };
    tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("图片加载已取消"),result=read=>result}
}

pub(super) async fn execute(client: &SftpSession, operation: Operation) -> Result<Output> {
    match operation {
        Operation::List(path) => {
            let path = client.canonicalize(path).await?;
            let entries = client.read_dir_limited(path.clone(), 20_000).await?;
            let mut rows = Vec::new();
            for entry in entries {
                ensure!(
                    rows.len() < 20_000,
                    "目录超过 20,000 项，请进入更小的子目录"
                );
                let name = entry.file_name();
                ensure!(
                    !name.contains(['/', '\0']) && name != "." && name != "..",
                    "远端目录返回无效文件名"
                );
                let meta = entry.metadata();
                rows.push(Entry {
                    name: entry.file_name(),
                    path: entry.path(),
                    directory: meta.is_dir(),
                    symlink: meta.is_symlink(),
                    size: meta.size.unwrap_or(0),
                    modified: meta.mtime.unwrap_or(0),
                    permissions: meta.permissions.unwrap_or(0),
                });
            }
            rows.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            Ok(Output::Directory {
                path,
                entries: rows,
            })
        }
        Operation::Mkdir(path) => {
            client.create_dir(path).await?;
            Ok(Output::Done("目录已创建".into()))
        }
        Operation::Create(path) => {
            let mut file = client
                .open_with_flags(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                )
                .await?;
            file.shutdown().await?;
            Ok(Output::Done("文件已创建".into()))
        }
        Operation::Rename { from, to } => {
            client.rename(from, to).await?;
            Ok(Output::Done("重命名完成".into()))
        }
        Operation::Remove { path, directory } => {
            if directory {
                remove_tree(client, &path).await?;
            } else {
                client.remove_file(path).await?;
            }
            Ok(Output::Done("删除完成".into()))
        }
        Operation::ReadImage(request) => read_image(client, request).await,
        Operation::ReadDocumentImage { document, source } => {
            let path = crate::files::document::image_path(&document, &source)?;
            read_image(
                client,
                crate::files::images::Request::new(path, crate::files::images::Mode::Document),
            )
            .await
        }
        Operation::ReadText(path) => {
            let file = client.open(path.clone()).await?;
            let mut bytes = Vec::new();
            file.take((TEXT_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .await?;
            ensure!(
                bytes.len() <= TEXT_LIMIT,
                "文本编辑器最多打开 8 MiB 文件，请下载后编辑"
            );
            Ok(Output::Text {
                path,
                text: String::from_utf8(bytes).context("远端文件不是 UTF-8 文本")?,
            })
        }
        Operation::WriteText { path, text } => {
            ensure!(text.len() <= TEXT_LIMIT, "文本编辑器最多保存 8 MiB");
            let mut file = client.create(path).await?;
            file.write_all(text.as_bytes()).await?;
            file.shutdown().await?;
            Ok(Output::Done("文件已保存".into()))
        }
        Operation::Upload { local, remote } => {
            let total = transfer::upload(client, local, remote).await?;
            Ok(Output::Done(format!("上传完成：{total} 字节")))
        }
        Operation::Download { remote, local } => {
            let total = transfer::download(client, remote, local).await?;
            Ok(Output::Done(format!("下载完成：{total} 字节")))
        }
        _ => anyhow::bail!("不是 SFTP 文件操作"),
    }
}
async fn remove_tree(client: &SftpSession, root: &str) -> Result<()> {
    ensure!(
        root != "/" && root != "." && !root.is_empty(),
        "不能删除远端根目录"
    );
    let mut pending = vec![(root.to_string(), 0_usize, false)];
    let mut visited = 0;
    while let Some((path, depth, done)) = pending.pop() {
        ensure!(
            depth <= 64 && visited <= 100_000,
            "删除目录超过深度或条目限制"
        );
        if done {
            client.remove_dir(path).await?;
            continue;
        }
        let metadata = client.symlink_metadata(path.clone()).await?;
        if metadata.is_dir() && !metadata.is_symlink() {
            pending.push((path.clone(), depth, true));
            for entry in client.read_dir_limited(path, 20_000).await? {
                let name = entry.file_name();
                ensure!(
                    !name.contains(['/', '\0']) && name != "." && name != "..",
                    "远端目录返回无效文件名"
                );
                visited += 1;
                pending.push((entry.path(), depth + 1, false));
            }
        } else {
            client.remove_file(path).await?;
        }
    }
    Ok(())
}

async fn upload_file(
    client: &SftpSession,
    local: std::path::PathBuf,
    remote: String,
) -> Result<u64> {
    upload_file_progress(client, local, remote, None).await
}
async fn upload_file_progress(
    client: &SftpSession,
    local: std::path::PathBuf,
    remote: String,
    job: Option<&crate::files::object_copy::Job>,
) -> Result<u64> {
    let mut input = tokio::task::spawn_blocking(move || File::open(local)).await??;
    let mut output = client.create(remote).await?;
    let mut total = 0_u64;
    loop {
        let (file, bytes) = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
            let mut bytes = vec![0; CHUNK];
            let n = input.read(&mut bytes)?;
            bytes.truncate(n);
            Ok((input, bytes))
        })
        .await??;
        input = file;
        if bytes.is_empty() {
            break;
        }
        output.write_all(&bytes).await?;
        total += bytes.len() as u64;
        if let Some(job) = job {
            job.transferred(total);
        }
    }
    output.shutdown().await?;
    Ok(total)
}

pub(super) struct UploadTarget<'a>(pub &'a SftpSession);
impl crate::files::upload::Target for UploadTarget<'_> {
    fn object_storage(&self) -> bool {
        false
    }
    async fn mkdir(&self, remote: &str) -> Result<()> {
        match self.0.create_dir(remote.to_owned()).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let metadata = self.0.metadata(remote.to_owned()).await;
                if metadata.is_ok_and(|meta| meta.is_dir()) {
                    Ok(())
                } else {
                    Err(error.into())
                }
            }
        }
    }
    async fn write(
        &self,
        entry: &crate::files::upload::Entry,
        remote: &str,
        job: &std::sync::Arc<crate::files::object_copy::Job>,
    ) -> Result<u64> {
        upload_file_progress(self.0, entry.local.clone(), remote.to_owned(), Some(job)).await
    }
}

async fn download_file(
    client: &SftpSession,
    remote: String,
    local: std::path::PathBuf,
) -> Result<u64> {
    let mut input = client.open(remote).await?;
    let directory = local
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut output =
        tokio::task::spawn_blocking(move || tempfile::NamedTempFile::new_in(directory)).await??;
    let mut bytes = vec![0; CHUNK];
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut bytes).await?;
        if n == 0 {
            break;
        }
        let data = bytes[..n].to_vec();
        output = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
            output.write_all(&data)?;
            Ok(output)
        })
        .await??;
        total += n as u64;
    }
    tokio::task::spawn_blocking(move || -> Result<()> {
        output.as_file().sync_all()?;
        output.persist(local).map_err(|e| e.error)?;
        Ok(())
    })
    .await??;
    Ok(total)
}
