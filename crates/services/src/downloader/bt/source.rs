use super::*;
use futures_util::StreamExt as _;
pub(super) const MAX_METADATA: usize = 8 * 1024 * 1024;
pub(crate) enum Source {
    Magnet(String),
    File(PathBuf),
    Url(url::Url),
}
impl Source {
    pub(crate) fn parse(value: &str) -> Result<Option<Self>> {
        if value
            .get(..7)
            .is_some_and(|v| v.eq_ignore_ascii_case("magnet:"))
        {
            let value = format!("magnet:{}", &value[7..]);
            validate_selection(&value)?;
            let magnet = librqbit::Magnet::parse(&value)
                .map_err(|_| anyhow::anyhow!("磁力链缺少有效的 BT 信息哈希"))?;
            ensure!(
                magnet.as_id20().is_some(),
                "此下载器需要包含 BT v1 信息哈希的磁力链"
            );
            return Ok(Some(Self::Magnet(value)));
        }
        if let Ok(url) = url::Url::parse(value) {
            if url.scheme() == "file" {
                let path = url
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("种子文件路径无效"))?;
                ensure!(
                    path.extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("torrent")),
                    "请选择 .torrent 文件"
                );
                return Ok(Some(Self::File(path)));
            }
            if matches!(url.scheme(), "http" | "https")
                && url.path().to_ascii_lowercase().ends_with(".torrent")
            {
                return Ok(Some(Self::Url(url)));
            }
        }
        let path = PathBuf::from(value);
        if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("torrent"))
        {
            return Ok(Some(Self::File(path)));
        }
        Ok(None)
    }
    pub(super) fn selection(&self) -> Result<Option<Vec<usize>>> {
        if let Self::Magnet(value) = self {
            validate_selection(value)?;
            return Ok(librqbit::Magnet::parse(value)?.get_select_only());
        }
        Ok(None)
    }
    pub(super) fn expected_hash(&self) -> Result<Option<String>> {
        if let Self::Magnet(value) = self {
            validate_selection(value)?;
            return Ok(librqbit::Magnet::parse(value)?
                .as_id20()
                .map(|v| v.as_string()));
        }
        Ok(None)
    }
    pub(crate) fn name(&self) -> String {
        match self {
            Self::Magnet(value) => url::Url::parse(value)
                .ok()
                .and_then(|url| {
                    url.query_pairs()
                        .find(|(k, _)| k == "dn")
                        .map(|(_, v)| sanitize_name(&v))
                })
                .unwrap_or_else(|| "正在获取种子信息".into()),
            Self::File(path) => sanitize_name(
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("BT 下载"),
            ),
            Self::Url(url) => sanitize_name(
                url.path_segments()
                    .and_then(|mut v| v.next_back())
                    .unwrap_or("BT 下载"),
            ),
        }
    }
    pub(super) async fn load(self, client: &reqwest::Client) -> Result<AddTorrent<'static>> {
        match self {
            Self::Magnet(value) => {
                validate_selection(&value)?;
                Ok(AddTorrent::from_url(value))
            }
            Self::File(path) => Ok(AddTorrent::from_bytes(read_metadata(&path).await?)),
            Self::Url(url) => {
                let response = client
                    .get(url)
                    .send()
                    .await
                    .context("无法获取种子文件")?
                    .error_for_status()
                    .context("种子服务器返回错误")?;
                ensure!(
                    response
                        .content_length()
                        .is_none_or(|n| n <= MAX_METADATA as u64),
                    "种子文件超过 8 MiB"
                );
                let mut chunks = response.bytes_stream();
                let mut bytes = Vec::new();
                while let Some(chunk) = chunks.next().await {
                    let chunk = chunk.context("种子文件下载中断")?;
                    ensure!(
                        bytes.len() + chunk.len() <= MAX_METADATA,
                        "种子文件超过 8 MiB"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                Ok(AddTorrent::from_bytes(bytes))
            }
        }
    }
}

pub(super) async fn read_metadata(path: &std::path::Path) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt as _;
    let file = tokio::fs::File::open(path)
        .await
        .context("无法打开种子文件")?;
    ensure!(
        file.metadata().await?.len() <= MAX_METADATA as u64,
        "种子文件超过 8 MiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_METADATA as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= MAX_METADATA, "种子文件超过 8 MiB");
    Ok(bytes)
}

// BEP-53 ranges must be bounded before the upstream parser expands them.
fn validate_selection(value: &str) -> Result<()> {
    let url = url::Url::parse(value)?;
    let mut total = 0usize;
    for (_, selection) in url.query_pairs().filter(|(key, _)| key == "so") {
        for part in selection.split(',').filter(|s| !s.is_empty()) {
            let (start, end) = if let Some((start, end)) = part.split_once('-') {
                (start.parse::<usize>()?, end.parse::<usize>()?)
            } else {
                let index = part.parse::<usize>()?;
                (index, index)
            };
            ensure!(
                start <= end && end < 100_000,
                "磁力链选择的文件编号超出范围"
            );
            total = total.saturating_add(end - start + 1);
            ensure!(total <= 100_000, "磁力链选择的文件超过 100000 项");
        }
    }
    Ok(())
}
