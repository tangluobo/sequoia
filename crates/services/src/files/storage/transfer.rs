use super::*;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

impl Storage {
    pub(crate) async fn upload(&self, local: PathBuf, remote: String) -> Result<u64> {
        let mut pending = vec![(local, remote, 0)];
        let (mut count, mut total) = (0, 0);
        while let Some((local, remote, depth)) = pending.pop() {
            count += 1;
            ensure!(
                depth <= 64 && count <= 100_000,
                "上传目录超过深度或条目限制"
            );
            let metadata = tokio::fs::symlink_metadata(&local).await?;
            ensure!(!metadata.is_symlink(), "递归上传不跟随符号链接");
            if metadata.is_dir() {
                self.operator
                    .create_dir(&self.directory_key(&remote)?)
                    .await?;
                let mut entries = tokio::fs::read_dir(&local).await?;
                let mut children = 0;
                while let Some(entry) = entries.next_entry().await? {
                    children += 1;
                    ensure!(
                        children <= 20_000 && pending.len() < 100_000,
                        "目录超过条目限制"
                    );
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("文件名不是 UTF-8"))?;
                    pending.push((
                        entry.path(),
                        format!("{}/{name}", remote.trim_end_matches('/')),
                        depth + 1,
                    ));
                }
            } else {
                ensure!(metadata.is_file(), "仅支持普通文件和目录");
                total += self.upload_file(local, &remote).await?;
            }
        }
        Ok(total)
    }
    pub(crate) async fn upload_file(&self, local: PathBuf, remote: &str) -> Result<u64> {
        self.upload_file_progress(local, remote, None).await
    }
    pub(super) async fn upload_file_progress(
        &self,
        local: PathBuf,
        remote: &str,
        job: Option<std::sync::Arc<crate::files::object_copy::Job>>,
    ) -> Result<u64> {
        if self.bucket_root {
            return Box::pin(
                self.bucket(remote)?
                    .upload_file_progress(local, remote, job),
            )
            .await;
        }
        let mut input = tokio::fs::File::open(&local).await?;
        let chunk = self
            .operator
            .info()
            .capability()
            .write_multi_min_size
            .unwrap_or(64 * 1024)
            .max(64 * 1024);
        let key = self.key(remote)?;
        let mut options = self.operator.writer_with(&key).concurrent(1).chunk(chunk);
        let mime = mime_guess::from_path(&local)
            .first_or_octet_stream()
            .to_string();
        if crate::files::object_copy::is_object(&self.connection) {
            options = options.content_type(&mime);
        }
        let writer = options.await?;
        let mut writer = super::writer::UploadWriter::new(writer, job.clone());
        let mut buffer = vec![0; 64 * 1024];
        let mut total = 0;
        let result = async {
            loop {
                let n = input.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                writer.get().write(buffer[..n].to_vec()).await?;
                total += n as u64;
                if let Some(job) = &job {
                    job.transferred(total);
                }
            }
            writer.close().await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            writer.abort().await;
            return Err(error);
        }
        Ok(total)
    }
    pub(crate) async fn download(&self, remote: String, local: PathBuf) -> Result<u64> {
        let mut pending = vec![(remote, local, 0)];
        let (mut count, mut total) = (0, 0);
        while let Some((remote, local, depth)) = pending.pop() {
            count += 1;
            ensure!(
                depth <= 64 && count <= 100_000,
                "下载目录超过深度或条目限制"
            );
            let key = if remote.ends_with('/') {
                self.directory_key(&remote)?
            } else {
                self.key(&remote)?
            };
            let meta = self.metadata(&key).await?;
            if meta.is_dir() {
                tokio::fs::create_dir_all(&local).await?;
                let Output::Directory { entries, .. } = self.list(&remote).await? else {
                    unreachable!()
                };
                for entry in entries {
                    ensure!(
                        super::super::super::terminal::ssh::zmodem::safe_name(&entry.name),
                        "远端文件名不适用于本机"
                    );
                    ensure!(pending.len() < 100_000, "目录超过条目限制");
                    pending.push((entry.path, local.join(entry.name), depth + 1));
                }
            } else {
                total += self.download_file(&remote, local).await?;
            }
        }
        Ok(total)
    }
    pub(crate) async fn download_file(&self, remote: &str, local: PathBuf) -> Result<u64> {
        if self.bucket_root {
            return Box::pin(self.bucket(remote)?.download_file(remote, local)).await;
        }
        let parent = local.parent().context("下载目标缺少父目录")?.to_path_buf();
        tokio::fs::create_dir_all(&parent).await?;
        let (temporary, file) = tokio::task::spawn_blocking(move || -> Result<_> {
            let temporary = tempfile::NamedTempFile::new_in(parent)?;
            let file = temporary.reopen()?;
            Ok((temporary, file))
        })
        .await??;
        let mut output = tokio::fs::File::from_std(file);
        if self.connection.kind() == "FTP" {
            let total =
                super::ftp::download(&self.connection, &self.key(remote)?, &mut output).await?;
            output.sync_all().await?;
            drop(output);
            tokio::task::spawn_blocking(move || {
                temporary
                    .persist(local)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            })
            .await??;
            return Ok(total);
        }
        let mut reader = self
            .operator
            .reader_with(&self.key(remote)?)
            .chunk(64 * 1024)
            .concurrent(1)
            .await?
            .into_futures_async_read(..)
            .await?;
        let mut buffer = vec![0; 64 * 1024];
        let mut total = 0;
        loop {
            let n = futures_util::AsyncReadExt::read(&mut reader, &mut buffer).await?;
            if n == 0 {
                break;
            }
            output.write_all(&buffer[..n]).await?;
            total += n as u64;
        }
        output.sync_all().await?;
        drop(output);
        tokio::task::spawn_blocking(move || {
            temporary
                .persist(local)
                .map(|_| ())
                .map_err(anyhow::Error::from)
        })
        .await??;
        Ok(total)
    }
    async fn descendants(&self, prefix: &str) -> Result<Vec<(String, bool)>> {
        let mut lister = self
            .operator
            .lister_with(&format!("{}/", prefix.trim_end_matches('/')))
            .recursive(true)
            .await?;
        let mut entries = Vec::new();
        while let Some(entry) = lister.try_next().await? {
            ensure!(
                entries.len() < 100_000 && entry.path().matches('/').count() <= 64,
                "目录超过条目或深度限制"
            );
            entries.push((entry.path().to_owned(), entry.metadata().is_dir()));
        }
        Ok(entries)
    }
    pub(crate) async fn remove_directory(&self, path: &str) -> Result<()> {
        let key = self.key(path)?;
        let mut entries = self.descendants(&key).await?;
        entries.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        if self.connection.kind() == "FTP" {
            entries.retain(|(path, _)| path.trim_end_matches('/') != key);
            entries.push((key, true));
            return super::ftp::remove(&self.connection, &entries).await;
        }
        for (key, _) in entries {
            self.operator.delete(&key).await?;
        }
        self.operator.delete(&format!("{key}/")).await?;
        Ok(())
    }
    pub(crate) async fn rename(&self, from: &str, to: &str, directory: bool) -> Result<()> {
        ensure!(!to.starts_with(&format!("{from}/")), "不能将目录移入自身");
        if self.connection.kind() == "FTP" {
            return super::ftp::rename(&self.connection, from, to).await;
        }
        if self.operator.info().capability().rename {
            self.operator.rename(from, to).await?;
            return Ok(());
        }
        ensure!(self.operator.info().capability().copy, "此后端不支持重命名");
        if directory {
            let entries = self.descendants(from).await?;
            self.operator.create_dir(&format!("{to}/")).await?;
            for (source, is_dir) in &entries {
                let destination = format!(
                    "{to}{}",
                    source.strip_prefix(from).context("目录前缀不匹配")?
                );
                if *is_dir {
                    self.operator.create_dir(&destination).await?;
                } else {
                    self.operator.copy(source, &destination).await?;
                }
            }
            // Copy every object successfully before removing any source object.
            self.remove_directory(&self.display(from)).await?;
        } else {
            self.operator.copy(from, to).await?;
            self.operator.delete(from).await?;
        }
        Ok(())
    }
}
