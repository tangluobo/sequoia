use super::*;
use crate::files::object_copy::{self, Job, Request};
use std::sync::Arc;
use tokio::io::AsyncWriteExt as _;
struct Task {
    from: String,
    to: String,
    bytes: Option<u64>,
    marker: bool,
}
struct Plan {
    tasks: Vec<Task>,
    retained: usize,
}
impl Plan {
    fn push(&mut self, task: Task, job: &Job) -> Result<()> {
        for key in [&task.from, &task.to] {
            validate(key)?;
        }
        self.retained = self
            .retained
            .saturating_add(2 * std::mem::size_of::<Task>())
            .saturating_add(task.from.capacity())
            .saturating_add(task.to.capacity());
        ensure!(
            self.tasks.len() < 100_000 && self.retained <= 8 * 1024 * 1024,
            "对象复制计划超过100000项或8 MiB，请分批复制"
        );
        job.found(self.tasks.len() + 1, &task.from);
        self.tasks.push(task);
        Ok(())
    }
}
fn validate(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty()
            && !key.starts_with('/')
            && !key.contains('\0')
            && !key.split('/').any(|p| matches!(p, "." | "..")),
        "对象键含有无法安全寻址的路径段"
    );
    Ok(())
}
impl Storage {
    pub(crate) async fn copy_request(request: Request, job: &Arc<Job>) -> Result<()> {
        static LIMIT: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
        let limiter = LIMIT.get_or_init(|| tokio::sync::Semaphore::new(2));
        let _permit = loop {
            if job.is_cancelled() {
                return Ok(());
            }
            match limiter.try_acquire() {
                Ok(permit) => break permit,
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    job.waiting();
                    tokio::select! {_=job.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(200))=>{}}
                }
                Err(error) => return Err(error.into()),
            }
        };
        job.scanning();
        let Request {
            mut source,
            mut target,
            selection,
            bucket,
            prefix,
        } = request;
        selection.validate()?;
        ensure!(
            object_copy::is_object(&source) && object_copy::is_object(&target),
            "复制粘贴仅支持对象存储"
        );
        ensure!(
            source.id == selection.connection_id && source.kind() == selection.kind,
            "源连接类型已变化，请重新复制"
        );
        ensure!(
            !bucket.is_empty() && !bucket.contains(['/', '\\', '\0']),
            "请先进入一个 Bucket 再粘贴"
        );
        let server = source.id == target.id && source.kind() == target.kind();
        source.set("bucket", selection.bucket.clone());
        source.set("s3Prefix", String::new());
        target.set("bucket", bucket);
        target.set("s3Prefix", String::new());
        let (source, target) = tokio::select! {
            biased;
            _ = job.cancelled() => return Ok(()),
            result = async { tokio::try_join!(Storage::connect(&source), Storage::connect(&target)) } => result?,
        };
        let mut plan = Plan {
            tasks: Vec::new(),
            retained: 0,
        };
        for item in &selection.items {
            if job.is_cancelled() {
                return Ok(());
            }
            validate(&item.key)?;
            let destination = item.destination(&prefix);
            if item.directory {
                let from = format!("{}/", item.key.trim_end_matches('/'));
                let mut listing = tokio::select! {_=job.cancelled()=>return Ok(()),result=source.operator.lister_with(&from).recursive(true)=>result?};
                loop {
                    let next = tokio::select! {_=job.cancelled()=>return Ok(()),result=listing.try_next()=>result?};
                    let Some(entry) = next else {
                        break;
                    };
                    if job.is_cancelled() {
                        return Ok(());
                    }
                    let marker = entry.metadata().is_dir();
                    // Original OssService.listObjectsRecursive skips zero-byte
                    // directory placeholders; MinIO's recursive listing keeps them.
                    if marker && source.connection.kind() == "ALIYUN_OSS" {
                        continue;
                    }
                    let relative = entry
                        .path()
                        .strip_prefix(&from)
                        .context("对象列表返回了目录外的路径")?;
                    plan.push(
                        Task {
                            from: entry.path().into(),
                            to: format!("{destination}{relative}"),
                            bytes: Some(entry.metadata().content_length()),
                            marker,
                        },
                        job,
                    )?;
                }
            } else {
                plan.push(
                    Task {
                        from: item.key.clone(),
                        to: destination,
                        bytes: None,
                        marker: false,
                    },
                    job,
                )?;
            }
        }
        // Finish enumeration before writing, including copies into a child of
        // the source directory. New destination keys cannot re-enter the scan.
        let client = buckets::client()?;
        for task in &plan.tasks {
            if job.is_cancelled() {
                break;
            }
            job.begin_file(plan.tasks.len(), &task.from, task.bytes);
            let result = if server {
                target
                    .copy_on_server(&client, &selection.bucket, task)
                    .await
                    .map(|()| true)
            } else {
                source.copy_through_file(&target, task, job).await
            };
            match result {
                Ok(true) => job.completed_file(None),
                Ok(false) => break,
                Err(error) if job.is_cancelled() => {
                    let _ = error;
                    break;
                }
                Err(error) => job.completed_file(Some(&format!("{error:#}"))),
            }
        }
        Ok(())
    }
    async fn copy_on_server(
        &self,
        client: &reqwest::Client,
        source_bucket: &str,
        task: &Task,
    ) -> Result<()> {
        const PATH: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'/')
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let encoded = percent_encoding::utf8_percent_encode(&task.from, PATH);
        let mut headers = http::HeaderMap::new();
        headers.insert(
            if self.connection.kind() == "ALIYUN_OSS" {
                "x-oss-copy-source"
            } else {
                "x-amz-copy-source"
            },
            http::HeaderValue::from_str(&format!("/{source_bucket}/{encoded}"))?,
        );
        let body = buckets::send(
            &self.connection,
            client,
            self.route.as_deref(),
            buckets::Request {
                method: http::Method::PUT,
                bucket: Some(self.connection.text("bucket")),
                key: Some(&task.to),
                query: None,
                body: Vec::new(),
                headers,
            },
        )
        .await?;
        let mut reader = quick_xml::Reader::from_reader(body.as_slice());
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Start(element)
                | quick_xml::events::Event::Empty(element) => {
                    ensure!(
                        element.local_name().as_ref() == b"CopyObjectResult",
                        "对象复制被拒绝：{}",
                        String::from_utf8_lossy(&body[..body.len().min(4096)])
                    );
                    return Ok(());
                }
                quick_xml::events::Event::Eof => bail!("对象复制返回了空响应"),
                _ => {}
            }
        }
    }
    async fn copy_through_file(
        &self,
        target: &Storage,
        task: &Task,
        job: &Arc<Job>,
    ) -> Result<bool> {
        let result = {
            let transfer = self.copy_file_inner(target, task, job);
            tokio::pin!(transfer);
            tokio::select! {result=&mut transfer=>Some(result),_=job.cancelled()=>None}
        };
        if let Some(result) = result {
            result
        } else {
            job.wait_cleanup().await;
            Ok(false)
        }
    }
    async fn copy_file_inner(&self, target: &Storage, task: &Task, job: &Arc<Job>) -> Result<bool> {
        if task.marker {
            target.operator.create_dir(&task.to).await?;
            return Ok(true);
        }
        let temporary = tokio::task::spawn_blocking(tempfile::NamedTempFile::new).await??;
        let output = temporary.reopen()?;
        let mut output = tokio::fs::File::from_std(output);
        let size = self.operator.stat(&task.from).await?.content_length();
        job.transfer_stage("下载", Some(size));
        let mut reader = self
            .operator
            .reader_with(&task.from)
            .chunk(64 * 1024)
            .concurrent(1)
            .await?
            .into_futures_async_read(..)
            .await?;
        let mut buffer = vec![0; 64 * 1024];
        let mut downloaded = 0u64;
        loop {
            if job.is_cancelled() {
                return Ok(false);
            }
            let count = futures_util::AsyncReadExt::read(&mut reader, &mut buffer).await?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).await?;
            downloaded = downloaded.saturating_add(count as u64);
            job.transferred(downloaded);
        }
        output.sync_all().await?;
        drop(output);
        drop(reader);
        if job.is_cancelled() {
            return Ok(false);
        }
        job.transfer_stage("上传", Some(downloaded));
        let mut input = tokio::fs::File::open(temporary.path()).await?;
        let chunk = target
            .operator
            .info()
            .capability()
            .write_multi_min_size
            .unwrap_or(64 * 1024)
            .max(64 * 1024);
        let writer = target
            .operator
            .writer_with(&task.to)
            .chunk(chunk)
            .concurrent(1)
            .await?;
        let mut writer = super::writer::UploadWriter::new(writer, Some(job.clone()));
        let mut uploaded = 0u64;
        let result = async {
            loop {
                if job.is_cancelled() {
                    return Ok(false);
                }
                let count = tokio::io::AsyncReadExt::read(&mut input, &mut buffer).await?;
                if count == 0 {
                    break;
                }
                writer.get().write(buffer[..count].to_vec()).await?;
                uploaded = uploaded.saturating_add(count as u64);
                job.transferred(uploaded);
            }
            writer.close().await?;
            Ok::<_, anyhow::Error>(true)
        }
        .await;
        if !matches!(result, Ok(true)) {
            writer.abort().await;
        }
        drop(input);
        drop(temporary);
        result
    }
}
