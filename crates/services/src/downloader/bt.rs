//! Native BitTorrent adapter; allocated only while a BT queue job is running.
mod legacy;
mod source;
mod target;
#[cfg(test)]
mod tests;
use super::*;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions};
pub(super) use source::Source;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
struct Payload {
    path: PathBuf,
    name: String,
    files: Option<std::collections::BTreeSet<PathBuf>>,
}

struct Lifetime(CancellationToken);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) async fn run(
    source: Source,
    row: &Arc<Mutex<Record>>,
    control: &crate::backup::Control,
    revision: &AtomicU64,
    wake: &Wake,
    client: &reqwest::Client,
) -> Result<PathBuf> {
    crate::init_crypto();
    let record = row.lock().unwrap().clone();
    let token = CancellationToken::new();
    let options = SessionOptions {
        dht: Some(librqbit::DhtSessionConfig {
            persistence: None,
            ..Default::default()
        }),
        persistence: None,
        cancellation_token: Some(token.clone()),
        concurrent_init_limit: Some(1),
        peer_limit: Some(record.requested_threads.clamp(1, 32)),
        client_name_and_version: Some("Tomato".into()),
        ..Default::default()
    };
    execute(
        source,
        row,
        control,
        revision,
        wake,
        client,
        options,
        Vec::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    source: Source,
    row: &Arc<Mutex<Record>>,
    control: &crate::backup::Control,
    revision: &AtomicU64,
    wake: &Wake,
    client: &reqwest::Client,
    mut options: SessionOptions,
    peers: Vec<std::net::SocketAddr>,
) -> Result<PathBuf> {
    let record = row.lock().unwrap().clone();
    let target = target::Target::open(&record).await?;
    let token = options
        .cancellation_token
        .get_or_insert_with(CancellationToken::new)
        .clone();
    let _lifetime = Lifetime(token.clone());
    let session = tokio::select! {
        biased;
        _ = control.wait_cancelled() => anyhow::bail!("已取消"),
        session = Session::new_with_opts(target.stage.clone(), options) => session?,
    };
    let result = tokio::select! {
        biased;
        _ = control.wait_cancelled() => Err(anyhow::anyhow!("已取消")),
        result = transfer(&session, source, &target, row, revision, wake, client, peers) => result,
    };
    // Stop sockets and release file handles off the UI thread before publishing
    // the final target. Cancellation also interrupts unresolved magnet metadata.
    token.cancel();
    session.stop().await;
    drop(session);
    row.lock().unwrap().actual_threads = 0;
    let payload = result?;
    row.lock().unwrap().status = "BT · 正在保存已校验的文件".into();
    changed(revision, wake);
    let cancellation = control.clone();
    let destination = tokio::task::spawn_blocking(move || {
        target.commit(
            &payload.path,
            &payload.name,
            payload.files.as_ref(),
            &cancellation,
        )
    })
    .await??;
    row.lock().unwrap().target_file = Some(destination.display().to_string());
    Ok(destination)
}

#[allow(clippy::too_many_arguments)]
async fn transfer(
    session: &Arc<Session>,
    source: Source,
    target: &target::Target,
    row: &Arc<Mutex<Record>>,
    revision: &AtomicU64,
    wake: &Wake,
    client: &reqwest::Client,
    peers: Vec<std::net::SocketAddr>,
) -> Result<Payload> {
    row.lock().unwrap().status = "BT · 正在获取种子信息".into();
    changed(revision, wake);
    let selection = source.selection()?;
    let expected_hash = source.expected_hash()?;
    let input = match source::read_metadata(&target.metadata).await {
        Ok(bytes) if bytes.len() <= source::MAX_METADATA => AddTorrent::from_bytes(bytes),
        _ => source.load(client).await?,
    };
    let listed = session
        .add_torrent(
            input,
            Some(AddTorrentOptions {
                list_only: true,
                only_files: selection,
                initial_peers: Some(peers.clone()),
                ..Default::default()
            }),
        )
        .await?;
    let AddTorrentResponse::ListOnly(listed) = listed else {
        anyhow::bail!("BT 元数据返回了不匹配的结果");
    };
    ensure!(
        expected_hash.is_none_or(|expected| expected == listed.info_hash.as_string()),
        "缓存种子与磁力链信息哈希不匹配"
    );
    ensure!(
        listed.torrent_bytes.len() <= source::MAX_METADATA,
        "种子信息超过 8 MiB"
    );
    let name = listed
        .info
        .name()
        .map(|name| sanitize_name(&name))
        .unwrap_or_else(|| "BT 下载".into());
    ensure!(
        listed.output_folder.starts_with(&target.stage),
        "种子输出目录超出任务目录"
    );
    let mut first = None;
    let mut count = 0;
    let mut files = Vec::new();
    let mut names = std::collections::HashSet::new();
    for file in listed.info.iter_file_details_ext() {
        let path = file.details.filename.to_pathbuf();
        target::validate_relative(&path)?;
        ensure!(
            names.insert(path.to_string_lossy().to_lowercase()),
            "种子存在 Windows 同名文件"
        );
        first.get_or_insert(path);
        files.push((file.details.filename.to_pathbuf(), file.details.len));
        count += 1;
        ensure!(count <= 100_000, "种子文件超过 100000 项");
    }
    let first = first.context("种子不包含文件")?;
    let payload = if listed.output_folder == target.stage {
        ensure!(count == 1, "多文件种子缺少根目录");
        listed.output_folder.join(first)
    } else {
        listed.output_folder.clone()
    };
    target::validate_relative(payload.strip_prefix(&target.stage)?)?;
    let publication = listed
        .only_files
        .as_ref()
        .map(|indices| {
            ensure!(!indices.is_empty(), "未选择种子文件");
            indices
                .iter()
                .map(|index| {
                    let (relative, _) = files.get(*index).context("种子文件选择索引无效")?;
                    Ok(listed
                        .output_folder
                        .join(relative)
                        .strip_prefix(&payload)?
                        .to_owned())
                })
                .collect::<Result<std::collections::BTreeSet<_>>>()
        })
        .transpose()?;
    let legacy_record = row.lock().unwrap().clone();
    legacy::seed(
        target,
        &legacy_record,
        &listed.output_folder,
        &payload,
        &files,
    )
    .await?;
    tokio::fs::write(&target.metadata, &listed.torrent_bytes).await?;
    let mut initial_peers = peers;
    initial_peers.extend(listed.seen_peers);
    initial_peers.sort_unstable();
    initial_peers.dedup();
    let peer_limit = row.lock().unwrap().requested_threads.clamp(1, 32);
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(listed.torrent_bytes),
            Some(AddTorrentOptions {
                overwrite: true,
                only_files: listed.only_files,
                initial_peers: Some(initial_peers),
                peer_limit: Some(peer_limit),
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("BT 任务没有启动")?;
    {
        let mut row = row.lock().unwrap();
        row.file_name = name.clone();
        row.target_file = Some(payload.display().to_string());
    }
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    loop {
        interval.tick().await;
        let stats = handle.stats();
        if let Some(error) = stats.error {
            anyhow::bail!("BT 下载失败：{error}");
        }
        {
            let mut row = row.lock().unwrap();
            row.downloaded_bytes = stats.progress_bytes;
            row.total_bytes = i64::try_from(stats.total_bytes).context("BT 文件总长度超出范围")?;
            row.actual_threads = stats
                .live
                .as_ref()
                .map_or(0, |live| live.snapshot.peer_stats.live as usize);
            row.status = if matches!(
                stats.state,
                librqbit::TorrentStatsState::Initializing { .. }
            ) {
                "BT · 正在校验本地分片".into()
            } else {
                format!("BT · {} 连接 · 下载并校验分片", row.actual_threads)
            };
        }
        changed(revision, wake);
        if stats.finished {
            break;
        }
    }
    session.delete(handle.id().into(), false).await?;
    Ok(Payload {
        path: payload,
        name,
        files: publication,
    })
}
