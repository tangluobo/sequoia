use super::*;

pub(super) enum Source {
    Http(url::Url),
    Ed2k(tomato_ed2k::types::Ed2kFileLink),
    Bt(super::bt::Source),
}

impl Source {
    pub(super) fn parse(value: &str) -> Result<Self> {
        ensure!(value.len() <= 8192, "下载地址超过 8 KiB");
        let value = value.trim();
        ensure!(!value.is_empty(), "请输入下载地址");
        if let Some(source) = super::bt::Source::parse(value)? {
            return Ok(Self::Bt(source));
        }
        if value
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("ed2k://"))
        {
            ensure!(
                value
                    .get(..13)
                    .is_some_and(|s| s.eq_ignore_ascii_case("ed2k://|file|")),
                "请输入 eD2k 文件链接，不能使用服务器或服务器列表链接"
            );
            ensure!(value.ends_with("|/"), "eD2k 文件链接缺少结尾 |/");
            let fields: Vec<_> = value[13..].split('|').collect();
            ensure!(
                fields.len() >= 4,
                "eD2k 文件链接缺少文件名、长度或 MD4 校验值"
            );
            ensure!(!fields[0].is_empty(), "eD2k 文件名不能为空");
            let size: u64 = fields[1].parse().context("eD2k 文件长度无效")?;
            ensure!(
                (1..=0x4000000000).contains(&size),
                "eD2k 文件长度必须为 1 字节至 256 GiB"
            );
            ensure!(
                fields[2].len() == 32 && fields[2].bytes().all(|b| b.is_ascii_hexdigit()),
                "eD2k MD4 校验值必须为 32 位十六进制字符"
            );
            let mut link = tomato_ed2k::parse_ed2k_link(value)
                .map_err(|_| anyhow::anyhow!("eD2k 文件链接无效"))?;
            link.file_name = sanitize_name(&link.file_name);
            ensure!(
                link.file_name.encode_utf16().count() <= 240,
                "下载文件名过长"
            );
            return Ok(Self::Ed2k(link));
        }
        let url = url::Url::parse(value).map_err(|_| anyhow::anyhow!("下载地址无效"))?;
        ensure!(
            matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
            "支持 HTTP/HTTPS、磁力链、.torrent 和 eD2k 文件链接"
        );
        Ok(Self::Http(url))
    }

    pub(super) fn record(self, url: String, directory: PathBuf, threads: usize) -> Record {
        let (protocol, file_name, total_bytes) = match self {
            Self::Http(parsed) => {
                let file = parsed
                    .path_segments()
                    .and_then(|mut parts| parts.next_back())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("download");
                (
                    "HTTP",
                    sanitize_name(&percent_encoding::percent_decode_str(file).decode_utf8_lossy()),
                    -1,
                )
            }
            Self::Ed2k(link) => ("ED2K", link.file_name, link.file_size as i64),
            Self::Bt(source) => ("BITTORRENT", source.name(), -1),
        };
        Record {
            url: url.trim().to_owned(),
            protocol: protocol.into(),
            directory: directory.display().to_string(),
            requested_threads: threads.clamp(1, 32),
            file_name,
            total_bytes,
            ..Default::default()
        }
    }
}
