//! Account-level S3/OSS requests use the same native signers as object I/O.
use super::*;
use reqsign_core::{Context as SigningContext, Signer};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default)]
struct BucketResult {
    #[serde(rename = "Buckets")]
    buckets: Buckets,
    #[serde(rename = "IsTruncated")]
    truncated: bool,
    #[serde(rename = "NextMarker")]
    marker: String,
    #[serde(rename = "ContinuationToken")]
    token: String,
    #[serde(rename = "NextContinuationToken")]
    next_token: String,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Buckets {
    #[serde(rename = "Bucket")]
    entries: Vec<Bucket>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Bucket {
    #[serde(rename = "Name")]
    name: String,
}

pub(super) async fn request(
    c: &Connection,
    route: Option<&route::Route>,
    method: http::Method,
    bucket: Option<&str>,
    query: Option<(&str, &str)>,
    body: Vec<u8>,
) -> Result<Vec<u8>> {
    let client = client()?;
    send(
        c,
        &client,
        route,
        Request {
            method,
            bucket,
            key: None,
            query,
            body,
            headers: http::HeaderMap::new(),
        },
    )
    .await
}
pub(super) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
pub(super) struct Request<'a> {
    pub method: http::Method,
    pub bucket: Option<&'a str>,
    pub key: Option<&'a str>,
    pub query: Option<(&'a str, &'a str)>,
    pub body: Vec<u8>,
    pub headers: http::HeaderMap,
}
pub(super) async fn send(
    c: &Connection,
    client: &reqwest::Client,
    route: Option<&route::Route>,
    request: Request<'_>,
) -> Result<Vec<u8>> {
    let Request {
        method,
        bucket,
        key,
        query,
        body,
        headers,
    } = request;
    let oss = c.kind() == "ALIYUN_OSS";
    let mut url = url::Url::parse(&endpoint(c, oss))?;
    if let Some(bucket) = bucket {
        ensure!(
            !bucket.is_empty() && !bucket.contains(['/', '\\', '\0']),
            "Bucket 名无效"
        );
        if path_style(c) {
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("端点不支持路径"))?
                .push(bucket);
        } else {
            let host = url.host_str().context("端点缺少主机")?;
            url.set_host(Some(&format!("{bucket}.{host}")))?;
        }
    }
    if let Some((key, value)) = query {
        url.query_pairs_mut().append_pair(key, value);
    }
    if let Some(key) = key {
        ensure!(
            bucket.is_some()
                && !key.is_empty()
                && !key.contains('\0')
                && !key.split('/').any(|part| matches!(part, "." | "..")),
            "对象键含有无法安全寻址的路径段"
        );
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("端点不支持对象路径"))?
            .pop_if_empty()
            .extend(key.split('/'));
    }
    let mut parts = http::Request::builder()
        .method(method)
        .uri(url.as_str())
        .body(())?
        .into_parts()
        .0;
    parts.headers.extend(headers);
    parts.headers.insert(
        http::header::HOST,
        http::HeaderValue::from_str(parts.uri.authority().context("存储端点缺少主机")?.as_str())?,
    );
    if !body.is_empty() {
        parts.headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/xml"),
        );
    }
    if !c.text("username").is_empty() || !c.text("password").is_empty() {
        if oss {
            Signer::new(
                SigningContext::new(),
                reqsign_aliyun_oss::StaticCredentialProvider::new(
                    c.text("username"),
                    c.text("password"),
                ),
                reqsign_aliyun_oss::RequestSigner::new(bucket.unwrap_or_default()),
            )
            .sign(&mut parts, None)
            .await?;
        } else {
            let region = if c.text("region").is_empty() {
                "us-east-1"
            } else {
                c.text("region")
            };
            Signer::new(
                SigningContext::new(),
                reqsign_aws_v4::StaticCredentialProvider::new(
                    c.text("username"),
                    c.text("password"),
                ),
                reqsign_aws_v4::RequestSigner::new("s3", region),
            )
            .sign(&mut parts, None)
            .await?;
        }
    }
    let client = route.map_or(client, |route| &route.client);
    let url = if let Some(route) = route {
        route.checked_url(&parts.uri.to_string())?
    } else {
        url
    };
    let mut response = client
        .request(parts.method, url)
        .headers(parts.headers)
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "Bucket 响应超过 2 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    ensure!(
        status.is_success(),
        "对象存储返回 {status}：{}",
        String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
    );
    Ok(bytes)
}
pub(super) async fn list(c: &Connection, route: Option<&route::Route>) -> Result<Output> {
    let mut entries = Vec::new();
    let mut marker = String::new();
    loop {
        let query = (!marker.is_empty()).then_some((
            if c.kind() == "ALIYUN_OSS" {
                "marker"
            } else {
                "continuation-token"
            },
            marker.as_str(),
        ));
        let body = request(c, route, http::Method::GET, None, query, Vec::new()).await?;
        let listing: BucketResult =
            quick_xml::de::from_reader(body.as_slice()).context("无法解析 Bucket 列表")?;
        for bucket in listing.buckets.entries {
            ensure!(entries.len() < 20_000, "Bucket 列表超过 20,000 项");
            ensure!(
                !bucket.name.is_empty() && !bucket.name.contains(['/', '\0']),
                "服务返回无效 Bucket 名"
            );
            entries.push(Entry {
                path: format!("/{}/", bucket.name),
                name: bucket.name,
                directory: true,
                symlink: false,
                size: 0,
                modified: 0,
                permissions: 0,
            });
        }
        let next = if !listing.marker.is_empty() {
            listing.marker
        } else if !listing.next_token.is_empty() {
            listing.next_token
        } else {
            listing.token
        };
        if next.is_empty() {
            ensure!(!listing.truncated, "Bucket 列表未提供后续分页标记");
            break;
        }
        ensure!(next != marker, "Bucket 分页标记未前进");
        marker = next;
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Output::Directory {
        path: "/".into(),
        entries,
    })
}
impl Storage {
    pub(crate) fn bucket(&self, path: &str) -> Result<Self> {
        let key = self.key(path)?;
        let bucket = key
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .context("请先选择 Bucket")?;
        let mut c = self.connection.clone();
        c.set("bucket", bucket.to_owned());
        c.set("useSshTunnel", false);
        let mut child = Self::new(&c)?;
        child.connection = self.connection.clone();
        child.connection.set("bucket", bucket.to_owned());
        child.route = self.route.clone();
        child.apply_route();
        child.display_root = format!("/{bucket}");
        // Reuse at most four per-bucket HTTP pools. External edit caches belong
        // to the root session, so evicting an operator cannot delete open files.
        let mut cache = self
            .bucket_operators
            .lock()
            .map_err(|_| anyhow::anyhow!("Bucket 缓存锁失效"))?;
        if let Some(index) = cache.iter().position(|(name, _)| name == bucket) {
            let entry = cache.remove(index);
            child.operator = entry.1.clone();
            cache.push(entry);
        } else {
            cache.push((bucket.to_owned(), child.operator.clone()));
            if cache.len() > 4 {
                cache.remove(0);
            }
        }
        Ok(child)
    }
    pub(super) async fn root_operation(&self, operation: Operation) -> Result<Output> {
        match operation {
            Operation::List(path) if self.key(&path)?.is_empty() => self.list(&path).await,
            Operation::Mkdir(path) if self.key(&path)?.split('/').count() == 1 => {
                let bucket = self.key(&path)?;
                ensure!(!bucket.is_empty(), "Bucket 名不能为空");
                let region = self.connection.text("region");
                let body = if self.connection.kind() != "ALIYUN_OSS"
                    && !region.is_empty()
                    && region != "us-east-1"
                {
                    format!("<CreateBucketConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><LocationConstraint>{}</LocationConstraint></CreateBucketConfiguration>",quick_xml::escape::escape(region)).into_bytes()
                } else {
                    Vec::new()
                };
                request(
                    &self.connection,
                    self.route.as_deref(),
                    http::Method::PUT,
                    Some(&bucket),
                    None,
                    body,
                )
                .await?;
                Ok(Output::Done("Bucket 已创建".into()))
            }
            Operation::Remove {
                path,
                directory: true,
            } if self.key(&path)?.split('/').count() == 1 => {
                let bucket = self.key(&path)?;
                ensure!(!bucket.is_empty(), "不能删除存储根节点");
                request(
                    &self.connection,
                    self.route.as_deref(),
                    http::Method::DELETE,
                    Some(&bucket),
                    None,
                    Vec::new(),
                )
                .await?;
                Ok(Output::Done("Bucket 已删除".into()))
            }
            Operation::PrepareEdit { remote, launch } => {
                self.editors.lock().await.open(self, remote, launch).await
            }
            Operation::ClipboardFiles => Ok(Output::ClipboardFiles(
                tokio::task::spawn_blocking(super::super::clipboard::paths).await??,
            )),
            operation => {
                let path = match &operation {
                    Operation::List(p)
                    | Operation::Mkdir(p)
                    | Operation::Create(p)
                    | Operation::ReadText(p) => p,
                    Operation::ReadImage(request) => &request.path,
                    Operation::ExportFile(request) => &request.path,
                    Operation::Rename { from, .. } => from,
                    Operation::Remove { path, .. } | Operation::WriteText { path, .. } => path,
                    Operation::Upload { remote, .. } | Operation::Download { remote, .. } => remote,
                    _ => bail!("此存储节点不支持此操作"),
                };
                let child = self.bucket(path)?;
                Box::pin(child.execute(operation)).await
            }
        }
    }
}
