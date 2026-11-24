//! Internet Archive Live Music Archive search and FLAC downloads.
use crate::backup::Control;
use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, stream};
use reqwest::{Client, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

mod catalog;
mod playback;
pub use catalog::CatalogTrack;
pub use playback::Playback;

const ITEMS_PER_PAGE: u32 = 12;
const MAX_TRACKS: usize = 200;
const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_FILE: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchBy {
    #[default]
    Any,
    Song,
    Artist,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    #[default]
    Live,
    Public,
    PublicOrCatalog,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Track {
    pub item: String,
    pub file: String,
    pub title: String,
    pub artist: String,
    pub recording: String,
    pub bytes: u64,
    pub sha1: Option<String>,
}

impl Track {
    fn retained_bytes(&self) -> usize {
        self.item.capacity()
            + self.file.capacity()
            + self.title.capacity()
            + self.artist.capacity()
            + self.recording.capacity()
            + self.sha1.as_ref().map_or(0, String::capacity)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Request {
    Search {
        by: SearchBy,
        query: String,
        #[serde(default)]
        page: u32,
        #[serde(default)]
        scope: SearchScope,
    },
    CatalogSearch {
        query: String,
    },
    Download {
        tracks: Vec<Track>,
        directory: PathBuf,
    },
    Preview {
        track: Track,
        local_file: Option<PathBuf>,
    },
}

impl Request {
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Search { query, .. } | Self::CatalogSearch { query } => query.capacity(),
            Self::Download { tracks, directory } => {
                directory.capacity()
                    + tracks.capacity() * std::mem::size_of::<Track>()
                    + tracks.iter().map(Track::retained_bytes).sum::<usize>()
            }
            Self::Preview { track, local_file } => {
                track.retained_bytes() + local_file.as_ref().map_or(0, PathBuf::capacity)
            }
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Output {
    Search {
        tracks: Vec<Track>,
        page: u32,
        more: bool,
        unavailable: usize,
    },
    Catalog {
        tracks: Vec<CatalogTrack>,
    },
    Download {
        outcomes: Vec<Download>,
        cancelled: bool,
    },
    Preview {
        #[serde(skip)]
        playback: Playback,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum Download {
    Saved(PathBuf),
    Failed(String),
}

pub async fn execute(request: Request, control: Control) -> Result<Output> {
    let client = Client::builder()
        .user_agent(concat!("Tomato/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let from_catalog =
                attempt.previous().first().and_then(Url::host_str) == Some("itunes.apple.com");
            let permitted_host = url.host_str().is_some_and(|host| {
                if from_catalog {
                    host == "itunes.apple.com"
                } else {
                    host == "archive.org" || host.ends_with(".archive.org")
                }
            });
            if attempt.previous().len() < 5 && url.scheme() == "https" && permitted_host {
                attempt.follow()
            } else {
                attempt.error("音乐服务返回了无效的重定向")
            }
        }))
        .build()?;
    let archive = Archive {
        client,
        base: Url::parse("https://archive.org/")?,
    };
    archive.run(request, &control).await
}

struct Archive {
    client: Client,
    base: Url,
}

#[derive(Deserialize)]
struct SearchResponse {
    response: SearchPage,
}

#[derive(Deserialize)]
struct SearchPage {
    #[serde(rename = "numFound")]
    total: u32,
    docs: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    identifier: String,
}

#[derive(Deserialize)]
struct Item {
    metadata: Recording,
    #[serde(default)]
    files: Vec<AudioFile>,
}

#[derive(Deserialize)]
struct Recording {
    title: String,
    #[serde(default)]
    creator: Creator,
    #[serde(rename = "access-restricted-item")]
    restricted: Option<serde_json::Value>,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum Creator {
    One(String),
    Many(Vec<String>),
    #[default]
    Missing,
}

#[derive(Deserialize)]
struct AudioFile {
    name: String,
    title: Option<String>,
    #[serde(default)]
    artist: Creator,
    size: Option<String>,
    sha1: Option<String>,
    private: Option<serde_json::Value>,
}

impl Archive {
    async fn run(&self, request: Request, control: &Control) -> Result<Output> {
        match request {
            Request::Search {
                by,
                query,
                page,
                scope,
            } => {
                tokio::select! {
                    biased;
                    _ = control.wait_cancelled() => anyhow::bail!("搜索已取消"),
                    result = self.search(by, &query, page, scope) => result,
                }
            }
            Request::CatalogSearch { query } => {
                tokio::select! {
                    biased;
                    _ = control.wait_cancelled() => anyhow::bail!("搜索已取消"),
                    result = catalog::search(&self.client, &query) => Ok(Output::Catalog { tracks: result? }),
                }
            }
            Request::Download { tracks, directory } => {
                ensure!(
                    !tracks.is_empty() && tracks.len() <= MAX_TRACKS,
                    "请选择 1–{MAX_TRACKS} 首歌曲"
                );
                ensure!(!directory.as_os_str().is_empty(), "请选择保存目录");
                if control.cancelled() {
                    return Ok(Output::Download {
                        outcomes: Vec::new(),
                        cancelled: true,
                    });
                }
                tokio::fs::create_dir_all(&directory)
                    .await
                    .context("无法创建保存目录")?;
                let total = tracks.len();
                control.start(total as u64);
                let mut outcomes = Vec::with_capacity(total);
                for track in tracks {
                    if control.cancelled() {
                        break;
                    }
                    let outcome = self.save(&track, &directory, control).await;
                    match outcome {
                        Ok(path) => outcomes.push(Download::Saved(path)),
                        Err(_) if control.cancelled() => break,
                        Err(error) => outcomes.push(Download::Failed(format!("{error:#}"))),
                    }
                    control.object();
                }
                let cancelled = outcomes.len() < total;
                Ok(Output::Download {
                    outcomes,
                    cancelled,
                })
            }
            Request::Preview { track, local_file } => {
                control.check()?;
                let (path, temporary) = if let Some(path) = local_file
                    && tokio::fs::try_exists(&path).await?
                {
                    (path, None)
                } else {
                    let directory = tempfile::Builder::new()
                        .prefix("tomato-music-preview-")
                        .tempdir()?;
                    let path = self.save(&track, directory.path(), control).await?;
                    (path, Some(directory))
                };
                Ok(Output::Preview {
                    playback: Playback::start(path, temporary, control.clone()).await?,
                })
            }
        }
    }

    async fn search(
        &self,
        by: SearchBy,
        keyword: &str,
        page: u32,
        scope: SearchScope,
    ) -> Result<Output> {
        ensure!(
            keyword.chars().count() <= 100 && page < 1000,
            "搜索词或页码超出范围"
        );
        let query = words(keyword);
        ensure!(!query.is_empty(), "请输入歌名或歌手");
        let fields = match by {
            SearchBy::Song => format!(r#"(title:"{query}" OR description:"{query}")"#),
            SearchBy::Artist if scope == SearchScope::Live => format!(r#"creator:"{query}""#),
            SearchBy::Any | SearchBy::Artist => {
                format!(r#"(creator:"{query}" OR title:"{query}" OR description:"{query}")"#)
            }
        };
        let collection = if scope == SearchScope::Live {
            "collection:etree"
        } else {
            "mediatype:audio"
        };
        let q = format!(
            "{collection} AND format:FLAC AND NOT access-restricted-item:true AND {fields}"
        );
        let mut url = self.url(["advancedsearch.php"]);
        url.query_pairs_mut()
            .append_pair("q", &q)
            .append_pair("fl[]", "identifier")
            .append_pair("rows", &ITEMS_PER_PAGE.to_string())
            .append_pair("page", &(page + 1).to_string())
            .append_pair("output", "json");
        let SearchResponse { response } = get_json(&self.client, url)
            .await
            .context("搜索现场录音库失败")?;
        let mut seen = HashSet::new();
        let hits = response
            .docs
            .into_iter()
            .take(ITEMS_PER_PAGE as usize)
            .filter(|hit| valid_item(&hit.identifier) && seen.insert(hit.identifier.clone()));
        let items = stream::iter(hits)
            .map(|hit| async move {
                let item: Item =
                    get_json(&self.client, self.url(["metadata", &hit.identifier])).await?;
                Ok::<_, anyhow::Error>((hit.identifier, item))
            })
            .buffered(3);
        futures_util::pin_mut!(items);
        let mut tracks = Vec::new();
        let mut unavailable = 0;
        while let Some(item) = items.next().await {
            match item {
                Ok((identifier, item)) => {
                    tracks.extend(
                        item.tracks(&identifier, &query, by, scope)
                            .take(MAX_TRACKS - tracks.len()),
                    );
                }
                Err(_) => unavailable += 1,
            }
            if tracks.len() == MAX_TRACKS {
                break;
            }
        }
        if by != SearchBy::Artist {
            tracks.sort_by_cached_key(|track| {
                let title = words(&track.title);
                if title == query {
                    0
                } else if title
                    .strip_prefix(&query)
                    .is_some_and(|suffix| suffix.starts_with(' '))
                {
                    1
                } else {
                    2
                }
            });
        }
        let more = page < 999 && (page + 1) * ITEMS_PER_PAGE < response.total;
        if scope == SearchScope::PublicOrCatalog
            && page == 0
            && tracks.is_empty()
            && unavailable == 0
            && !more
        {
            return Ok(Output::Catalog {
                tracks: catalog::search(&self.client, keyword).await?,
            });
        }
        Ok(Output::Search {
            tracks,
            page,
            more,
            unavailable,
        })
    }

    fn url<'a>(&self, segments: impl IntoIterator<Item = &'a str>) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("archive base URL")
            .pop_if_empty()
            .extend(segments);
        url
    }

    async fn save(&self, track: &Track, directory: &Path, control: &Control) -> Result<PathBuf> {
        ensure!(
            valid_item(&track.item) && valid_file(&track.file),
            "录音文件标识无效"
        );
        ensure!((43..=MAX_FILE).contains(&track.bytes), "录音文件大小无效");
        let url = self.url(
            ["download", &track.item]
                .into_iter()
                .chain(track.file.split('/')),
        );
        let mut response = tokio::select! {
            biased;
            _ = control.wait_cancelled() => anyhow::bail!("下载已取消"),
            response = self.client.get(url).timeout(Duration::from_secs(15 * 60)).send() => response?,
        }.error_for_status()?;
        ensure!(
            response.status() == StatusCode::OK,
            "服务器没有返回完整录音"
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|size| size == track.bytes),
            "录音文件大小与目录记录不符"
        );
        let (file, mut temporary) = tempfile::Builder::new()
            .prefix(".tomato-music-")
            .tempfile_in(directory)
            .context("无法创建下载临时文件")?
            .into_parts();
        let mut file = tokio::fs::File::from_std(file);
        let mut digest = Sha1::new();
        let mut bytes = 0u64;
        let mut header = Vec::with_capacity(42);
        while let Some(chunk) = next_chunk(&mut response, control).await? {
            bytes += chunk.len() as u64;
            ensure!(bytes <= track.bytes, "录音文件超过目录记录大小");
            if header.len() < 42 {
                header.extend_from_slice(&chunk[..chunk.len().min(42 - header.len())]);
                if header.len() == 42 {
                    ensure!(
                        header.starts_with(b"fLaC")
                            && header[4] & 0x7f == 0
                            && header[5..8] == [0, 0, 34],
                        "服务器未返回原生 FLAC 文件"
                    );
                }
            }
            digest.update(&chunk);
            file.write_all(&chunk).await?;
        }
        ensure!(bytes == track.bytes, "录音下载不完整");
        if let Some(expected) = &track.sha1 {
            ensure!(
                expected.eq_ignore_ascii_case(&format!("{:x}", digest.finalize())),
                "FLAC 文件校验失败"
            );
        }
        file.sync_all().await?;
        drop(file);
        let stem = format!("{} - {}", track.artist, track.title);
        let stem: String = stem.chars().take(56).collect();
        let stem = crate::downloader::sanitize_name(&stem);
        for index in 0..1000 {
            control.check()?;
            let name = if index == 0 {
                format!("{stem}.flac")
            } else {
                format!("{stem} ({index}).flac")
            };
            let target = directory.join(name);
            match temporary.persist_noclobber(&target) {
                Ok(()) => return Ok(target),
                Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                    temporary = error.path
                }
                Err(error) => return Err(error.error).context("无法保存 FLAC 文件"),
            }
        }
        anyhow::bail!("无法生成可用的下载文件名")
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: Url) -> Result<T> {
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(body.len() + chunk.len() <= MAX_JSON, "音乐目录响应过大");
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).context("音乐目录返回了无法读取的数据")
}

async fn next_chunk(response: &mut Response, control: &Control) -> Result<Option<bytes::Bytes>> {
    tokio::select! {
        biased;
        _ = control.wait_cancelled() => anyhow::bail!("下载已取消"),
        chunk = response.chunk() => Ok(chunk?),
    }
}

impl Item {
    fn tracks(
        self,
        identifier: &str,
        query: &str,
        by: SearchBy,
        scope: SearchScope,
    ) -> impl Iterator<Item = Track> {
        let artist = self.metadata.creator;
        let recording: String = self.metadata.title.chars().take(200).collect();
        let recording_name_matches = words(&recording).contains(query);
        let restricted = flag(&self.metadata.restricted);
        self.files.into_iter().filter_map(move |file| {
            if restricted || flag(&file.private) || !valid_file(&file.name) {
                return None;
            }
            let bytes = file.size?.parse::<u64>().ok()?;
            if !(43..=MAX_FILE).contains(&bytes) {
                return None;
            }
            let title: String = file
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| {
                    let name = file.name.rsplit('/').next().unwrap_or_default();
                    name[..name.len() - ".flac".len()].into()
                })
                .chars()
                .take(200)
                .collect();
            let song_matches = words(&title).contains(query);
            let artist_matches = if file.artist.first().is_empty() {
                artist.matches(query) || (artist.first().is_empty() && recording_name_matches)
            } else {
                file.artist.matches(query)
            };
            let matched = match by {
                SearchBy::Any => song_matches || artist_matches,
                SearchBy::Song => song_matches,
                SearchBy::Artist => scope == SearchScope::Live || artist_matches,
            };
            if !matched {
                return None;
            }
            let credited_artist = file.artist.first();
            let credited_artist = if credited_artist.is_empty() {
                artist.first()
            } else {
                credited_artist
            };
            Some(Track {
                item: identifier.to_owned(),
                file: file.name,
                title,
                artist: credited_artist.chars().take(200).collect(),
                recording: recording.clone(),
                bytes,
                sha1: file.sha1,
            })
        })
    }
}

impl Creator {
    fn first(&self) -> &str {
        match self {
            Self::One(name) => name,
            Self::Many(names) => names.first().map_or("", String::as_str),
            Self::Missing => "",
        }
    }

    fn matches(&self, query: &str) -> bool {
        match self {
            Self::One(name) => words(name).contains(query),
            Self::Many(names) => names.iter().any(|name| words(name).contains(query)),
            Self::Missing => false,
        }
    }
}

fn valid_item(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn valid_file(name: &str) -> bool {
    name.len() <= 600
        && name.to_ascii_lowercase().ends_with(".flac")
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && name
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn words(value: &str) -> String {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn flag(value: &Option<serde_json::Value>) -> bool {
    value
        .as_ref()
        .is_some_and(|value| value == true || value.as_str() == Some("true"))
}

#[cfg(test)]
mod tests;
