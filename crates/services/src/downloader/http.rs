use super::*;
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE,
};
use std::io::{Seek, SeekFrom, Write};

#[derive(Serialize, Deserialize)]
struct Resume {
    #[serde(default)]
    owner: String,
    identity: String,
    validator: String,
    length: u64,
    chunk: u64,
    complete: Vec<bool>,
}
struct Segments {
    manifest: Resume,
    path: PathBuf,
}
fn network_error(_: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!("下载网络请求失败，请检查连接或重试")
}
fn range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.strip_prefix("bytes ")?;
    let (positions, total) = value.split_once('/')?;
    let (start, end) = positions.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}
fn sidecar(target: &std::path::Path, suffix: &str) -> PathBuf {
    let mut value = target.as_os_str().to_owned();
    value.push(suffix);
    value.into()
}
fn identity(url: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(ring::digest::digest(&ring::digest::SHA256, url.as_bytes()))
}
fn suggested_name(header: &str) -> Option<String> {
    if header.len() > 8192 {
        return None;
    }
    let mut parts = Vec::new();
    let (mut quoted, mut escaped, mut start) = (false, false, 0);
    for (i, ch) in header.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == ';' && !quoted {
            parts.push(&header[start..i]);
            start = i + 1;
        }
    }
    parts.push(&header[start..]);
    let mut plain = None;
    for part in parts {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        let value = value.trim();
        if key.eq_ignore_ascii_case("filename*") {
            let mut fields = value.trim_matches('"').splitn(3, '\'');
            if fields
                .next()
                .is_some_and(|v| v.eq_ignore_ascii_case("UTF-8"))
            {
                let _ = fields.next();
                if let Some(value) = fields.next()
                    && let Ok(value) = percent_encoding::percent_decode_str(value).decode_utf8()
                {
                    return Some(super::sanitize_name(&value));
                }
            }
        } else if key.eq_ignore_ascii_case("filename") {
            let mut value = value.trim_matches('"').chars();
            let mut decoded = String::new();
            while let Some(ch) = value.next() {
                if ch == '\\' {
                    if let Some(next) = value.next() {
                        decoded.push(next);
                    }
                } else {
                    decoded.push(ch);
                }
            }
            plain = Some(super::sanitize_name(&decoded));
        }
    }
    plain
}
fn reserve(record: &Record, suggested: Option<String>) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let directory = PathBuf::from(&record.directory);
    std::fs::create_dir_all(&directory)?;
    let name = if record.target_file.is_some() {
        record.file_name.clone()
    } else {
        suggested.unwrap_or_else(|| record.file_name.clone())
    };
    let path = std::path::Path::new(&name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let suffix = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    for index in 0..10000 {
        let name = if index == 0 {
            name.clone()
        } else {
            format!("{stem} ({index}){suffix}")
        };
        let target = directory.join(name);
        if target.exists() {
            continue;
        }
        let part = sidecar(&target, ".tomato-rust.part");
        let manifest = sidecar(&part, ".json");
        if manifest.exists() {
            if record.target_file.as_deref().map(std::path::Path::new) != Some(target.as_path()) {
                continue;
            }
            ensure!(
                std::fs::metadata(&manifest)?.len() <= 2 * 1024 * 1024,
                "分片记录过大"
            );
            let previous: Resume = serde_json::from_reader(std::fs::File::open(&manifest)?)?;
            if previous.owner != record.id || previous.identity != identity(&record.url) {
                continue;
            }
            if part.exists() {
                return Ok((target, part, manifest));
            }
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
        {
            Ok(_) => return Ok((target, part, manifest)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("无法生成可用的下载文件名")
}
fn persist_segments(state: &Segments) -> Result<()> {
    tomato_core::config::atomic_write(&state.path, &serde_json::to_vec(&state.manifest)?)?;
    Ok(())
}
fn finish(part: &std::path::Path, target: &std::path::Path) -> Result<PathBuf> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(part)?;
    file.sync_all()?;
    let temporary =
        tempfile::NamedTempFile::from_parts(file, tempfile::TempPath::try_from_path(part)?);
    if let Err(error) = temporary.persist_noclobber(target) {
        let cause = error.error.to_string();
        let _ = error.file.keep();
        anyhow::bail!("保存下载失败（保留分片文件）：{cause}");
    }
    Ok(target.to_owned())
}
pub(super) async fn run(
    client: &reqwest::Client,
    row: &Arc<Mutex<Record>>,
    control: &crate::backup::Control,
    revision: &Arc<AtomicU64>,
    wake: &Wake,
) -> Result<PathBuf> {
    let record = row.lock().unwrap().clone();
    let parsed = url::Url::parse(&record.url)?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "该下载协议尚未接入原生引擎"
    );
    let name = std::path::Path::new(&record.file_name);
    ensure!(
        name.file_name()
            .is_some_and(|n| n == record.file_name.as_str())
            && !matches!(record.file_name.as_str(), "." | ".."),
        "下载文件名无效"
    );
    let mut probe = client
        .get(&record.url)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(network_error)?;
    if probe.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        drop(probe);
        probe = client
            .get(&record.url)
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(network_error)?;
    }
    let probe = probe.error_for_status().map_err(network_error)?;
    let partial = probe.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let strong_etag = probe
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.starts_with("W/"));
    let response_range = probe
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(range);
    let validator = probe
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.starts_with("W/"))
        .or_else(|| {
            probe
                .headers()
                .get(LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("")
        .to_owned();
    let length = if partial {
        let (start, end, length) = response_range.context("服务器分段响应缺少有效范围")?;
        ensure!(
            start == 0 && end == 0 && length > 0,
            "服务器分段响应范围无效"
        );
        Some(length)
    } else {
        probe.content_length()
    };
    let suggested = probe
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(suggested_name);
    let (target, part, manifest_path) = reserve(&record, suggested)?;
    {
        let mut record = row.lock().unwrap();
        record.file_name = target.file_name().unwrap().to_string_lossy().into_owned();
        record.total_bytes = length.and_then(|v| i64::try_from(v).ok()).unwrap_or(-1);
        record.target_file = Some(target.display().to_string());
        record.downloaded_bytes = 0;
        record.status = "下载中".into();
        record.actual_threads = if partial {
            record.requested_threads.clamp(1, 32)
        } else {
            1
        };
    }
    changed(revision, wake);
    if !partial {
        // Sequential responses cannot safely resume. Keep a marker so a later retry
        // can distinguish our partial file from an unrelated file.
        let manifest = Segments {
            path: manifest_path.clone(),
            manifest: Resume {
                owner: record.id.clone(),
                identity: identity(&record.url),
                validator: String::new(),
                length: length.unwrap_or(0),
                chunk: 0,
                complete: vec![],
            },
        };
        if manifest_path.exists() {
            let previous: Resume = serde_json::from_reader(std::fs::File::open(&manifest_path)?)?;
            ensure!(
                previous.identity == manifest.manifest.identity,
                "已有分片来自其他下载地址"
            );
        }
        persist_segments(&manifest)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&part)?;
        let mut response = probe;
        let mut downloaded = 0;
        let mut last = std::time::Instant::now();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            control.check()?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            row.lock().unwrap().downloaded_bytes = downloaded;
            if last.elapsed() >= std::time::Duration::from_millis(200) {
                changed(revision, wake);
                last = std::time::Instant::now();
            }
        }
        ensure!(
            length.is_none_or(|size| size == downloaded),
            "下载长度与服务器声明不一致"
        );
        file.sync_all()?;
        drop(file);
        control.check()?;
        let output = finish(&part, &target)?;
        let _ = std::fs::remove_file(manifest_path);
        return Ok(output);
    }
    drop(probe);
    let length = length.unwrap();
    let chunk = 1_048_576u64.max(length.div_ceil(65536));
    let count = length.div_ceil(chunk) as usize;
    let mut manifest = Resume {
        owner: record.id.clone(),
        identity: identity(&record.url),
        validator: validator.clone(),
        length,
        chunk,
        complete: vec![false; count],
    };
    if manifest_path.exists() {
        ensure!(
            std::fs::metadata(&manifest_path)?.len() <= 2 * 1024 * 1024,
            "分片记录过大"
        );
        let previous: Resume = serde_json::from_reader(std::fs::File::open(&manifest_path)?)?;
        ensure!(
            previous.identity == manifest.identity,
            "已有分片来自其他下载地址"
        );
        if !validator.is_empty()
            && previous.validator == validator
            && previous.length == length
            && previous.chunk == chunk
            && previous.complete.len() == count
            && std::fs::metadata(&part).is_ok_and(|m| m.len() == length)
        {
            manifest = previous;
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&part)?;
    file.set_len(length)?;
    drop(file);
    let completed_bytes = manifest
        .complete
        .iter()
        .enumerate()
        .filter(|(_, done)| **done)
        .map(|(i, _)| chunk.min(length - i as u64 * chunk))
        .sum();
    row.lock().unwrap().downloaded_bytes = completed_bytes;
    let segments = Arc::new(Mutex::new(Segments {
        manifest,
        path: manifest_path.clone(),
    }));
    persist_segments(&segments.lock().unwrap())?;
    let next = Arc::new(AtomicU64::new(0));
    let mut workers = tokio::task::JoinSet::new();
    let workers_count = record.requested_threads.clamp(1, 32).min(count);
    row.lock().unwrap().actual_threads = workers_count;
    for _ in 0..workers_count {
        let (client, row, control, revision, wake, segments, next, part, url, validator) = (
            client.clone(),
            row.clone(),
            control.clone(),
            revision.clone(),
            wake.clone(),
            segments.clone(),
            next.clone(),
            part.clone(),
            record.url.clone(),
            validator.clone(),
        );
        workers.spawn(async move {
            let mut file = std::fs::OpenOptions::new().write(true).open(part)?;
            loop {
                control.check()?;
                let index = next.fetch_add(1, Ordering::Relaxed) as usize;
                if index >= count {
                    break;
                }
                if segments.lock().unwrap().manifest.complete[index] {
                    continue;
                }
                let start = index as u64 * chunk;
                let end = (start + chunk).min(length) - 1;
                let mut request = client
                    .get(&url)
                    .header(ACCEPT_ENCODING, "identity")
                    .header(RANGE, format!("bytes={start}-{end}"));
                if !validator.is_empty() {
                    request = request.header(IF_RANGE, &validator);
                }
                let mut response = request
                    .send()
                    .await
                    .map_err(network_error)?
                    .error_for_status()
                    .map_err(network_error)?;
                ensure!(
                    response.status() == reqwest::StatusCode::PARTIAL_CONTENT
                        && response
                            .headers()
                            .get(CONTENT_RANGE)
                            .and_then(|v| v.to_str().ok())
                            .and_then(range)
                            == Some((start, end, length)),
                    "服务器分段范围变化，请重试下载"
                );
                if let Some(value) =
                    response
                        .headers()
                        .get(if strong_etag { ETAG } else { LAST_MODIFIED })
                    && !validator.is_empty()
                {
                    ensure!(
                        value.to_str().ok() == Some(validator.as_str()),
                        "远端文件在下载过程中发生变化"
                    );
                }
                file.seek(SeekFrom::Start(start))?;
                let mut downloaded = 0;
                let mut last = std::time::Instant::now();
                while let Some(bytes) = response.chunk().await.map_err(network_error)? {
                    control.check()?;
                    ensure!(
                        downloaded + bytes.len() as u64 <= end - start + 1,
                        "服务器返回超出分段的数据"
                    );
                    file.write_all(&bytes)?;
                    downloaded += bytes.len() as u64;
                    row.lock().unwrap().downloaded_bytes += bytes.len() as u64;
                    if last.elapsed() >= std::time::Duration::from_millis(200) {
                        changed(&revision, &wake);
                        last = std::time::Instant::now();
                    }
                }
                ensure!(downloaded == end - start + 1, "分段数据不完整");
                file.sync_data()?;
                {
                    let mut state = segments.lock().unwrap();
                    state.manifest.complete[index] = true;
                    persist_segments(&state)?;
                }
                changed(&revision, &wake);
            }
            Ok::<_, anyhow::Error>(())
        });
    }
    while let Some(result) = workers.join_next().await {
        result??;
    }
    control.check()?;
    let output = finish(&part, &target)?;
    let _ = std::fs::remove_file(manifest_path);
    Ok(output)
}
