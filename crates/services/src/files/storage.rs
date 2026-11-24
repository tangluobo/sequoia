//! Native storage adapter. No file or network work runs in an egui frame.
mod buckets;
mod copy;
mod copy_transfer;
mod document_image;
mod editor;
mod export;
mod ftp;
mod route;
mod transfer;
mod upload_target;
mod writer;
use super::operations::{Entry, Operation, Output};
use anyhow::{Context, Result, bail, ensure};
use futures_util::{AsyncReadExt, TryStreamExt};
use opendal::{Operator, services};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tomato_core::Connection;

pub struct Storage {
    pub(crate) operator: Operator,
    display_root: String,
    connection: Connection,
    pub(crate) bucket_root: bool,
    bucket_operators: std::sync::Mutex<Vec<(String, Operator)>>,
    initial_listing: std::sync::Mutex<Option<Output>>,
    initial_directory: Option<String>,
    pub(crate) editors: tokio::sync::Mutex<editor::Editors>,
    copy_context: super::object_copy::Context,
    route: Option<std::sync::Arc<route::Route>>,
    session_credentials: super::object_copy::sessions::Lease,
}
/// Call after stopping file actors, while their Tokio runtime is still alive.
pub async fn wait_upload_cleanup() {
    writer::wait_cleanup().await;
}
impl Storage {
    pub fn new(connection: &Connection) -> Result<Self> {
        ensure!(
            !connection.flag("useSshTunnel"),
            "SSH 存储连接需要异步建立隧道"
        );
        crate::init_crypto();
        opendal::install_default();
        let mut display_root = String::new();
        let bucket_root = (matches!(connection.kind(), "S3" | "ALIYUN_OSS")
            || connection.kind() == "LOCAL_DIRECTORY" && connection.text("directoryType") == "S3")
            && connection.text("bucket").is_empty();
        let bucket = if bucket_root {
            "tomato-root-placeholder"
        } else {
            connection.text("bucket")
        };
        let operator = match connection.kind() {
            "LOCAL_DIRECTORY" if connection.text("directoryType") != "S3" => {
                let path = PathBuf::from(connection.text("localDirectoryPath"));
                ensure!(path.is_absolute(), "本地目录需要绝对路径");
                let root = path.ancestors().last().context("本地目录路径无效")?;
                display_root = root
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches('/')
                    .to_owned();
                Operator::new(services::Fs::default().root(&root.to_string_lossy()))?
            }
            "FTP" => Operator::new(
                services::Ftp::default()
                    .endpoint(&format!(
                        "ftp://{}:{}",
                        connection.text("host"),
                        connection.number("port").max(1)
                    ))
                    .root("/")
                    .user(connection.text("username"))
                    .password(connection.text("password")),
            )?,
            "ALIYUN_OSS" => {
                let endpoint = endpoint(connection, true);
                Operator::new(
                    services::Oss::default()
                        .endpoint(&endpoint)
                        .bucket(bucket)
                        .root(connection.text("s3Prefix"))
                        .access_key_id(connection.text("username"))
                        .access_key_secret(connection.text("password"))
                        .addressing_style(if connection.flag("pathStyleAccess") {
                            "path"
                        } else {
                            "virtual"
                        }),
                )?
            }
            "S3" | "LOCAL_DIRECTORY" => {
                let endpoint = endpoint(connection, false);
                let region = if connection.text("region").is_empty() {
                    "us-east-1"
                } else {
                    connection.text("region")
                };
                let mut builder = services::S3::default()
                    .endpoint(&endpoint)
                    .region(region)
                    .bucket(bucket)
                    .root(connection.text("s3Prefix"))
                    .access_key_id(connection.text("username"))
                    .secret_access_key(connection.text("password"))
                    .disable_config_load()
                    .disable_ec2_metadata();
                if !path_style(connection) {
                    builder = builder.enable_virtual_host_style();
                }
                Operator::new(builder)?
            }
            kind => bail!("不支持的文件后端：{kind}"),
        };
        let operator = operator
            .layer(opendal::layers::ConcurrentLimitLayer::new(4))
            .layer(opendal::layers::TimeoutLayer::new().with_io_timeout(Duration::from_secs(30)));
        Ok(Self {
            operator,
            display_root,
            connection: connection.clone(),
            bucket_root,
            bucket_operators: Default::default(),
            initial_listing: Default::default(),
            initial_directory: (connection.kind() == "LOCAL_DIRECTORY"
                && connection.text("directoryType") != "S3")
                .then(|| connection.text("localDirectoryPath").replace('\\', "/")),
            editors: Default::default(),
            copy_context: Default::default(),
            route: None,
            session_credentials: Default::default(),
        })
    }
    /// S3/OSS folders may consist entirely of a key prefix, without a marker object.
    pub(crate) async fn metadata(&self, key: &str) -> opendal::Result<opendal::Metadata> {
        match self.operator.stat(key).await {
            Err(error)
                if error.kind() == opendal::ErrorKind::NotFound
                    && (matches!(self.connection.kind(), "S3" | "ALIYUN_OSS")
                        || self.connection.kind() == "LOCAL_DIRECTORY"
                            && self.connection.text("directoryType") == "S3") =>
            {
                let mut lister = self
                    .operator
                    .lister_with(&format!("{}/", key.trim_end_matches('/')))
                    .limit(1)
                    .await?;
                if lister.try_next().await?.is_some() {
                    Ok(opendal::MetadataBuilder::dir().build())
                } else {
                    Err(error)
                }
            }
            result => result,
        }
    }
    pub(crate) async fn exists(&self, key: &str) -> Result<bool> {
        match self.metadata(key).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    pub async fn check(&self) -> Result<()> {
        if self.bucket_root {
            let listing = buckets::list(&self.connection, self.route.as_deref()).await?;
            *self
                .initial_listing
                .lock()
                .map_err(|_| anyhow::anyhow!("Bucket 缓存锁失效"))? = Some(listing);
        } else if let Some(path) = &self.initial_directory {
            let meta = self.operator.stat(&self.directory_key(path)?).await?;
            ensure!(meta.is_dir(), "本地目录连接的路径不是目录");
        } else {
            self.operator.check().await?;
        }
        Ok(())
    }
    pub(crate) fn key(&self, path: &str) -> Result<String> {
        let path = path.replace('\\', "/");
        let path = if !self.display_root.is_empty() {
            if path == "/" {
                Some("")
            } else {
                path.strip_prefix(&self.display_root)
            }
            .context("路径不在当前文件系统卷内")?
        } else {
            &path
        };
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    ensure!(parts.pop().is_some(), "路径越过根目录");
                }
                _ => {
                    ensure!(!part.contains('\0'), "路径含有无效字符");
                    parts.push(part);
                }
            }
        }
        Ok(parts.join("/"))
    }
    fn display(&self, key: &str) -> String {
        format!("{}/{}", self.display_root, key.trim_start_matches('/'))
    }
    pub(crate) fn directory_key(&self, path: &str) -> Result<String> {
        let key = self.key(path)?;
        Ok(if key.is_empty() {
            "/".into()
        } else {
            format!("{}/", key.trim_end_matches('/'))
        })
    }
    pub async fn list(&self, path: &str) -> Result<Output> {
        if self.bucket_root {
            if self.key(path)?.is_empty() {
                if let Some(listing) = self
                    .initial_listing
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Bucket 缓存锁失效"))?
                    .take()
                {
                    return Ok(listing);
                }
                return buckets::list(&self.connection, self.route.as_deref()).await;
            }
            return Box::pin(self.bucket(path)?.list(path)).await;
        }
        let key = self.directory_key(path)?;
        let mut listing = self.operator.lister(&key).await?;
        let mut entries = Vec::new();
        while let Some(entry) = listing.try_next().await? {
            if entry.path().trim_end_matches('/') == key.trim_matches('/') {
                continue;
            }
            ensure!(entries.len() < 20_000, "目录超过 20,000 项，请按子目录浏览");
            let meta = entry.metadata();
            // Fs/FTP return metadata with the listing; do not stat every object.
            entries.push(Entry {
                name: entry.name().trim_end_matches('/').to_owned(),
                path: self.display(entry.path()),
                directory: meta.is_dir(),
                symlink: false,
                size: meta.content_length(),
                modified: meta
                    .last_modified()
                    .map(|t| t.into_inner().as_second().clamp(0, u32::MAX as i64) as u32)
                    .unwrap_or(0),
                permissions: 0,
            });
        }
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(Output::Directory {
            path: self.display(key.trim_matches('/')),
            entries,
        })
    }
    pub async fn execute(&self, operation: Operation) -> Result<Output> {
        if let Operation::Batch(operations) = operation {
            ensure!(operations.len() <= 1000, "批量操作超过 1000 项");
            let count = operations.len();
            for (index, operation) in operations.into_iter().enumerate() {
                ensure!(
                    !matches!(operation, Operation::Batch(_)),
                    "不能嵌套批量操作"
                );
                self.single(operation)
                    .await
                    .with_context(|| format!("已完成 {index}/{count} 项，后续操作已停止"))?;
            }
            return Ok(Output::Done(format!("已完成 {count} 项操作")));
        }
        self.single(operation).await
    }
    async fn single(&self, operation: Operation) -> Result<Output> {
        match operation {
            Operation::UploadFiles {
                directory,
                paths,
                deferred,
                job,
            } => {
                let report = super::upload::run(self, &directory, paths, deferred, job).await;
                Ok(Output::Uploaded { directory, report })
            }
            Operation::CopyObjects { directory, paths } => {
                self.copy_objects(directory, paths).await
            }
            Operation::PasteObjects { directory, job } => self.paste_objects(directory, job).await,
            operation => self.storage_operation(operation).await,
        }
    }
    async fn storage_operation(&self, operation: Operation) -> Result<Output> {
        if let Operation::ReadDocumentImage { document, source } = operation {
            return self.read_document_image(&document, &source).await;
        }
        if self.bucket_root {
            return self.root_operation(operation).await;
        }
        match operation {
            Operation::List(path) => self.list(&path).await,
            Operation::Mkdir(path) => {
                self.operator
                    .create_dir(&self.directory_key(&path)?)
                    .await?;
                Ok(Output::Done("目录已创建".into()))
            }
            Operation::Create(path) => {
                let key = self.key(&path)?;
                ensure!(!key.is_empty(), "不能覆盖根目录");
                ensure!(!self.exists(&key).await?, "文件已存在");
                self.operator.write(&key, Vec::<u8>::new()).await?;
                Ok(Output::Done("文件已创建".into()))
            }
            Operation::Rename { from, to } => {
                let was_directory = from.ends_with('/');
                let from = self.key(&from)?;
                let to = self.key(&to)?;
                ensure!(!from.is_empty() && !to.is_empty(), "不能重命名根目录");
                let meta = self
                    .metadata(&if was_directory {
                        format!("{from}/")
                    } else {
                        from.clone()
                    })
                    .await;
                let meta = match meta {
                    Err(error)
                        if !was_directory && error.kind() == opendal::ErrorKind::NotFound =>
                    {
                        self.metadata(&format!("{from}/")).await?
                    }
                    result => result?,
                };
                ensure!(!self.exists(&to).await?, "目标已存在");
                if meta.is_dir() {
                    ensure!(!self.exists(&format!("{to}/")).await?, "目标目录已存在");
                }
                self.rename(&from, &to, meta.is_dir()).await?;
                Ok(Output::Done("重命名完成".into()))
            }
            Operation::Remove { path, directory } => {
                let key = self.key(&path)?;
                ensure!(!key.is_empty(), "不能删除根目录");
                if directory {
                    self.remove_directory(&path).await?;
                } else if self.connection.kind() == "FTP" {
                    ftp::remove(&self.connection, &[(key, false)]).await?;
                } else {
                    self.operator.delete(&key).await?;
                }
                Ok(Output::Done("删除完成".into()))
            }
            Operation::ReadImage(request) => self.read_image(request).await,
            Operation::ExportFile(request) => {
                request
                    .run(async {
                        request
                            .prepare(|path| async move {
                                match self.list(&path).await? {
                                    Output::Directory { entries, .. } => Ok(entries),
                                    _ => anyhow::bail!("目录列表返回了不匹配的结果"),
                                }
                            })
                            .await?;
                        let mut first = None;
                        for (index, item) in request.items().iter().enumerate() {
                            let asset = if item.directory {
                                request.directory_asset(index, &item.path).await?
                            } else {
                                self.export_file(&request, index, &item.path).await?
                            };
                            if first.is_none() {
                                first = Some(asset);
                            }
                        }
                        Ok(Output::ExportReady(
                            request.path.clone(),
                            first.context("拖出列表为空")?,
                        ))
                    })
                    .await
            }
            Operation::ReadText(path) => {
                let key = self.key(&path)?;
                if self.connection.kind() == "FTP" {
                    return Ok(Output::Text {
                        text: ftp::read_text(&self.connection, &key).await?,
                        path,
                    });
                }
                ensure!(
                    self.operator.stat(&key).await?.content_length() <= 8 * 1024 * 1024,
                    "文本文件超过 8 MiB"
                );
                let mut reader = self
                    .operator
                    .reader_with(&key)
                    .concurrent(1)
                    .chunk(64 * 1024)
                    .await?
                    .into_futures_async_read(..)
                    .await?
                    .take(8 * 1024 * 1024 + 1);
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).await?;
                ensure!(bytes.len() <= 8 * 1024 * 1024, "读取时文件增长超过 8 MiB");
                Ok(Output::Text {
                    path,
                    text: String::from_utf8(bytes)
                        .context("文件不是 UTF-8 文本，请使用外部应用打开")?,
                })
            }
            Operation::WriteText { path, text } => {
                ensure!(text.len() <= 8 * 1024 * 1024, "文本超过 8 MiB");
                self.operator
                    .write(&self.key(&path)?, text.into_bytes())
                    .await?;
                Ok(Output::Done("文件已保存".into()))
            }
            Operation::Upload { local, remote } => {
                let bytes = self.upload(local, remote).await?;
                Ok(Output::Done(format!("上传完成：{bytes} 字节")))
            }
            Operation::Download { remote, local } => {
                let bytes = self.download(remote, local).await?;
                Ok(Output::Done(format!("下载完成：{bytes} 字节")))
            }
            Operation::PrepareEdit { remote, launch } => {
                self.editors.lock().await.open(self, remote, launch).await
            }
            Operation::ClipboardFiles => Ok(Output::ClipboardFiles(
                tokio::task::spawn_blocking(super::clipboard::paths).await??,
            )),
            _ => bail!("此文件后端不支持 SSH 管理命令"),
        }
    }
}
fn endpoint(c: &Connection, oss: bool) -> String {
    let value = if !c.text("endpoint").is_empty() {
        c.text("endpoint").trim().to_owned()
    } else if oss {
        format!("https://oss-{}.aliyuncs.com", c.text("region"))
    } else {
        format!(
            "https://s3.{}.amazonaws.com",
            if c.text("region").trim().is_empty() {
                "us-east-1"
            } else {
                c.text("region").trim()
            }
        )
    };
    if value.starts_with("http://") || value.starts_with("https://") {
        value
    } else {
        format!("{}://{value}", if oss { "https" } else { "http" })
    }
}

// MinIO's Java client automatically uses path addressing for custom endpoints,
// including old configurations whose pathStyleAccess remained false.
fn path_style(c: &Connection) -> bool {
    if c.kind() == "ALIYUN_OSS" || c.flag("pathStyleAccess") {
        return c.flag("pathStyleAccess");
    }
    let host = url::Url::parse(&endpoint(c, false))
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    !(host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn"))
}
