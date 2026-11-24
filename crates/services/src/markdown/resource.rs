//! Resolve direct image addresses before dispatching relative reads to storage.
use crate::files::images::{MAX_BYTES, append, document as decode};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;

pub(crate) enum Source<'a> {
    Http(url::Url),
    File(PathBuf),
    Data(&'a str),
    Relative(&'a str),
}

impl<'a> Source<'a> {
    pub(crate) fn parse(source: &'a str) -> Result<Self> {
        let source = java_trim(source);
        ensure!(!source.is_empty(), "图片地址为空");
        ensure!(!source.contains('\0'), "图片地址含有无效字符");
        let (scheme, tail) = source.split_once(':').unwrap_or(("", ""));
        if scheme.eq_ignore_ascii_case("data") {
            return Ok(Self::Data(tail));
        }
        if scheme.eq_ignore_ascii_case("file") {
            let file =
                url::Url::parse(source).map_err(|_| anyhow::anyhow!("无效的 file 图片地址"))?;
            ensure!(file.host().is_none(), "file 图片地址只支持本机路径");
            let path = file
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("无效的 file 图片路径"))?;
            #[cfg(windows)]
            ensure!(
                matches!(path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))),
                "file 图片地址只支持本机路径"
            );
            return Ok(Self::File(path));
        }
        if tail.starts_with("//")
            && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        {
            let address =
                url::Url::parse(source).map_err(|_| anyhow::anyhow!("无效的 HTTP 图片地址"))?;
            return Ok(Self::Http(address));
        }
        if let [letter, b':', b'/' | b'\\', ..] = source.as_bytes()
            && letter.is_ascii_alphabetic()
        {
            let path = PathBuf::from(source);
            ensure!(path.is_absolute(), "当前系统无法打开 Windows 绝对图片路径");
            return Ok(Self::File(path));
        }
        ensure!(!source.contains("://"), "不支持的图片地址");
        Ok(Self::Relative(source))
    }

    pub(crate) async fn picture(self) -> Result<super::Picture> {
        decode(async {
            match self {
                Self::Http(address) => http(address).await,
                Self::File(path) => read_file(&path).await,
                Self::Data(data) => data_bytes(data),
                Self::Relative(_) => bail!("相对图片缺少文件上下文"),
            }
        })
        .await
    }
}

pub(super) async fn picture(document: &Path, source: &str) -> Result<super::Picture> {
    match Source::parse(source)? {
        Source::Relative(source) => {
            let source = relative_path(source);
            ensure!(
                !source.is_empty() && !source.contains('\0'),
                "图片路径为空或含有无效字符"
            );
            let parent = document.parent().context("相对图片缺少文件上下文")?;
            decode(read_file(&parent.join(source.as_ref()))).await
        }
        direct => direct.picture().await,
    }
}

async fn read_file(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        tokio::fs::metadata(path).await?.is_file(),
        "图片地址不是普通文件"
    );
    let file = tokio::fs::File::open(path).await?;
    let metadata = file.metadata().await?;
    ensure!(metadata.is_file(), "图片地址不是普通文件");
    ensure!(metadata.len() <= MAX_BYTES as u64, "图片超过 8 MiB");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= MAX_BYTES, "图片超过 8 MiB");
    Ok(bytes)
}

async fn http(address: url::Url) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut response = client
        .get(address)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?
        .error_for_status()
        .map_err(reqwest::Error::without_url)?;
    let length = response.content_length().unwrap_or(0);
    ensure!(length <= MAX_BYTES as u64, "图片超过 8 MiB");
    let mut bytes = Vec::with_capacity(length as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        append(&mut bytes, &chunk)?;
    }
    Ok(bytes)
}

fn data_bytes(data: &str) -> Result<Vec<u8>> {
    let (metadata, encoded) = data.split_once(',').context("无效的 data 图片地址")?;
    let encoded = encoded.split_once('#').map_or(encoded, |(value, _)| value);
    ensure!(encoded.len() <= MAX_BYTES * 4, "图片超过 8 MiB");
    let input = percent_encoding::percent_decode_str(encoded);
    if metadata
        .rsplit_once(';')
        .is_some_and(|(_, marker)| marker.eq_ignore_ascii_case("base64"))
    {
        let input: Cow<'_, [u8]> = if encoded
            .bytes()
            .any(|byte| byte == b'%' || byte.is_ascii_whitespace())
        {
            Cow::Owned(
                input
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .take(MAX_BYTES.div_ceil(3) * 4 + 1)
                    .collect(),
            )
        } else {
            Cow::Borrowed(encoded.as_bytes())
        };
        ensure!(input.len() <= MAX_BYTES.div_ceil(3) * 4, "图片超过 8 MiB");
        let engine = if input.ends_with(b"=") {
            &base64::engine::general_purpose::STANDARD
        } else {
            &base64::engine::general_purpose::STANDARD_NO_PAD
        };
        let bytes = engine
            .decode(input)
            .map_err(|_| anyhow::anyhow!("无效的 Base64 图片数据"))?;
        ensure!(bytes.len() <= MAX_BYTES, "图片超过 8 MiB");
        Ok(bytes)
    } else {
        let bytes: Vec<_> = input.take(MAX_BYTES + 1).collect();
        ensure!(bytes.len() <= MAX_BYTES, "图片超过 8 MiB");
        Ok(bytes)
    }
}

pub(crate) fn java_trim(value: &str) -> &str {
    value.trim_matches(|character| character <= ' ')
}

pub(crate) fn relative_path(source: &str) -> Cow<'_, str> {
    let source = java_trim(source);
    let source = &source[..source.find(['?', '#']).unwrap_or(source.len())];
    // Java URLDecoder keeps the entire original input if any % escape is invalid.
    if source.match_indices('%').any(|(offset, _)| {
        !source
            .as_bytes()
            .get(offset + 1..offset + 3)
            .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
    }) {
        return Cow::Borrowed(source);
    }
    percent_encoding::percent_decode_str(source).decode_utf8_lossy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_decode_percent_escaped_binary_and_unpadded_base64() -> Result<()> {
        assert_eq!(
            data_bytes("image/png,%89PNG%0D%0A+%FF#anchor")?,
            b"\x89PNG\r\n+\xff"
        );
        assert_eq!(data_bytes("image/png;base64,%2Bw%0A")?, b"\xfb");
        assert_eq!(relative_path("  a%20b+%ZZ.png?x#y  "), "a%20b+%ZZ.png");
        Ok(())
    }

    #[tokio::test]
    async fn http_images_keep_queries_and_bound_streams_without_exposing_the_url() -> Result<()> {
        use axum::{Router, body::Body, response::Response, routing::get};
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let serve = axum::serve(
            listener,
            Router::new().route(
                "/photo",
                get(|uri: axum::http::Uri| async move {
                    if uri.query() == Some("token=fixture-secret") {
                        Response::new(Body::from_stream(futures_util::stream::iter(
                            std::iter::repeat_n(
                                Ok::<_, std::convert::Infallible>(bytes::Bytes::from(
                                    vec![0; 64 * 1024],
                                )),
                                MAX_BYTES / (64 * 1024) + 1,
                            ),
                        )))
                    } else {
                        Response::builder().status(403).body(Body::empty()).unwrap()
                    }
                }),
            ),
        );
        let server = tokio::spawn(async move { serve.await });
        let too_large = http(url::Url::parse(&format!(
            "HTTP://{address}/photo?token=fixture-secret"
        ))?)
        .await;
        let forbidden = http(url::Url::parse(&format!(
            "http://{address}/photo?private=fixture-secret"
        ))?)
        .await;
        server.abort();
        assert!(too_large.unwrap_err().to_string().contains("8 MiB"));
        let error = forbidden.unwrap_err().to_string();
        assert!(
            error.contains("403") && !error.contains("fixture-secret"),
            "{error}"
        );
        Ok(())
    }
}
