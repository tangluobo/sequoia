use super::super::source::Source as DownloadSource;
use super::*;
use librqbit::{ListenerMode, ListenerOptions};

fn isolated_options(listen: bool) -> SessionOptions {
    SessionOptions {
        dht: None,
        persistence: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: listen.then(|| ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn torrent(name: &str, bytes: &[u8]) -> (Vec<u8>, String) {
    let mut hashes = Vec::new();
    for piece in bytes.chunks(16384) {
        hashes.extend_from_slice(&Sha1::digest(piece));
    }
    let mut info = format!(
        "d6:lengthi{}e4:name{}:{}12:piece lengthi16384e6:pieces{}:",
        bytes.len(),
        name.len(),
        name,
        hashes.len()
    )
    .into_bytes();
    info.extend_from_slice(&hashes);
    info.push(b'e');
    let hash = format!("{:x}", Sha1::digest(&info));
    let mut torrent = b"d4:info".to_vec();
    torrent.extend_from_slice(&info);
    torrent.push(b'e');
    (torrent, hash)
}
fn multi_torrent(name: &str, files: &[(&str, Vec<u8>)]) -> (Vec<u8>, String) {
    let mut data = Vec::new();
    let mut info = b"d5:filesl".to_vec();
    for (path, bytes) in files {
        info.extend_from_slice(format!("d6:lengthi{}e4:pathl", bytes.len()).as_bytes());
        for part in path.split('/') {
            info.extend_from_slice(format!("{}:{part}", part.len()).as_bytes());
        }
        info.extend_from_slice(b"ee");
        data.extend_from_slice(bytes);
    }
    info.extend_from_slice(
        format!(
            "e4:name{}:{name}12:piece lengthi16384e6:pieces{}:",
            name.len(),
            data.len().div_ceil(16384) * 20
        )
        .as_bytes(),
    );
    for piece in data.chunks(16384) {
        info.extend_from_slice(&Sha1::digest(piece));
    }
    info.push(b'e');
    let hash = format!("{:x}", Sha1::digest(&info));
    let mut metadata = b"d4:info".to_vec();
    metadata.extend(info);
    metadata.push(b'e');
    (metadata, hash)
}
#[tokio::test]
async fn native_multifile_torrent_keeps_empty_files_and_publishes_only_selected_magnet_files()
-> Result<()> {
    crate::init_crypto();
    let root = tempfile::tempdir()?;
    let files = vec![
        ("alpha.txt", b"first file".to_vec()),
        (
            "nested/中文.bin",
            (0..50123).map(|i| (i % 239) as u8).collect(),
        ),
        ("nested/empty.bin", vec![]),
    ];
    let name = "目录种子";
    let (metadata, hash) = multi_torrent(name, &files);
    let path = root.path().join("multi.torrent");
    std::fs::write(&path, &metadata)?;
    let seed = root.path().join("seed");
    for (file, bytes) in &files {
        let destination = seed.join(name).join(file);
        std::fs::create_dir_all(destination.parent().unwrap())?;
        std::fs::write(destination, bytes)?;
    }
    let session = Session::new_with_opts(seed, isolated_options(true)).await?;
    let _lifetime = Lifetime(session.cancellation_token().clone());
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(metadata),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("seed missing")?;
    tokio::time::timeout(Duration::from_secs(10), handle.wait_until_completed()).await??;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let wake: Wake = Arc::new(|| {});
    for (index, source) in [
        Source::File(path),
        Source::Magnet(format!("magnet:?xt=urn:btih:{hash}&so=1")),
    ]
    .into_iter()
    .enumerate()
    {
        let value = if index == 0 {
            "multi-file-fixture".into()
        } else {
            format!("magnet:?xt=urn:btih:{hash}&so=1")
        };
        let row = Arc::new(Mutex::new(Record {
            url: value,
            directory: root
                .path()
                .join(format!("out-{index}"))
                .display()
                .to_string(),
            ..Default::default()
        }));
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            execute(
                source,
                &row,
                &crate::backup::Control::default(),
                &AtomicU64::new(0),
                &wake,
                &client,
                isolated_options(false),
                vec![session.listen_addr().unwrap()],
            ),
        )
        .await??;
        assert_eq!(std::fs::read(output.join(files[1].0))?, files[1].1);
        if index == 0 {
            assert_eq!(std::fs::read(output.join(files[0].0))?, files[0].1);
            assert_eq!(std::fs::metadata(output.join(files[2].0))?.len(), 0);
        } else {
            assert!(!output.join(files[0].0).exists() && !output.join(files[2].0).exists());
        }
    }
    session.stop().await;
    Ok(())
}
#[tokio::test]
async fn legacy_partial_bt_data_resumes_from_a_real_peer_without_modifying_original_bytes()
-> Result<()> {
    crate::init_crypto();
    let root = tempfile::tempdir()?;
    let bytes: Vec<_> = (0..131097).map(|i| (i % 241) as u8).collect();
    let name = "partial.bin";
    let (metadata, _) = torrent(name, &bytes);
    let path = root.path().join("partial.torrent");
    std::fs::write(&path, &metadata)?;
    let seed = root.path().join("seed");
    std::fs::create_dir(&seed)?;
    std::fs::write(seed.join(name), &bytes)?;
    let session = Session::new_with_opts(seed, isolated_options(true)).await?;
    let _lifetime = Lifetime(session.cancellation_token().clone());
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(metadata),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("seed missing")?;
    tokio::time::timeout(Duration::from_secs(10), handle.wait_until_completed()).await??;
    let directory = root.path().join("out");
    std::fs::create_dir(&directory)?;
    let original = directory.join(name);
    let prefix = &bytes[..32768];
    std::fs::write(&original, prefix)?;
    let stamp = std::fs::metadata(&original)?.modified()?;
    let row = Arc::new(Mutex::new(Record {
        url: path.display().to_string(),
        directory: directory.display().to_string(),
        target_file: Some(original.display().to_string()),
        ..Default::default()
    }));
    let client = reqwest::Client::builder().no_proxy().build()?;
    let wake: Wake = Arc::new(|| {});
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        execute(
            Source::File(path),
            &row,
            &crate::backup::Control::default(),
            &AtomicU64::new(0),
            &wake,
            &client,
            isolated_options(false),
            vec![session.listen_addr().unwrap()],
        ),
    )
    .await??;
    assert_eq!(std::fs::read(output)?, bytes);
    assert_eq!(std::fs::read(&original)?, prefix);
    assert_eq!(std::fs::metadata(original)?.modified()?, stamp);
    session.stop().await;
    Ok(())
}
#[tokio::test]
async fn legacy_bt_completed_data_is_verified_without_peers_and_original_is_unchanged() -> Result<()>
{
    crate::init_crypto();
    let root = tempfile::tempdir()?;
    let name = "旧任务.bin";
    let bytes: Vec<_> = (0..65591).map(|i| (i % 239) as u8).collect();
    let (metadata, _) = torrent(name, &bytes);
    let path = root.path().join("source.torrent");
    std::fs::write(&path, metadata)?;
    let directory = root.path().join("out");
    std::fs::create_dir(&directory)?;
    let original = directory.join(name);
    std::fs::write(&original, &bytes)?;
    let before = std::fs::metadata(&original)?.modified()?;
    let mut record = DownloadSource::parse(&path.display().to_string())?.record(
        path.display().to_string(),
        directory.clone(),
        2,
    );
    record.target_file = Some(original.display().to_string());
    let row = Arc::new(Mutex::new(record));
    let client = reqwest::Client::builder().no_proxy().build()?;
    let wake: Wake = Arc::new(|| {});
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        execute(
            Source::File(path),
            &row,
            &crate::backup::Control::default(),
            &AtomicU64::new(0),
            &wake,
            &client,
            isolated_options(false),
            vec![],
        ),
    )
    .await??;
    assert_ne!(output, original);
    assert_eq!(std::fs::read(output)?, bytes);
    assert_eq!(std::fs::read(&original)?, bytes);
    assert_eq!(std::fs::metadata(original)?.modified()?, before);
    Ok(())
}
#[tokio::test]
async fn torrent_file_magnet_and_http_metadata_download_verified_bytes_without_overwrite()
-> Result<()> {
    crate::init_crypto();
    let root = tempfile::tempdir()?;
    let seed = root.path().join("seed");
    std::fs::create_dir(&seed)?;
    let bytes: Vec<_> = (0..197283).map(|i| (i % 251) as u8).collect();
    let name = "中文数据.bin";
    std::fs::write(seed.join(name), &bytes)?;
    let (metadata, hash) = torrent(name, &bytes);
    let torrent_path = root.path().join("local.torrent");
    std::fs::write(&torrent_path, &metadata)?;
    let seed_session = Session::new_with_opts(seed, isolated_options(true)).await?;
    let _lifetime = Lifetime(seed_session.cancellation_token().clone());
    let seeding = seed_session
        .add_torrent(
            AddTorrent::from_bytes(metadata.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("seed missing")?;
    tokio::time::timeout(Duration::from_secs(10), seeding.wait_until_completed()).await??;
    let address = seed_session
        .listen_addr()
        .context("seed listener missing")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let http_address = listener.local_addr()?;
    let app = axum::Router::new().route(
        "/fixture.torrent",
        axum::routing::get(move || {
            let metadata = metadata.clone();
            async move { metadata }
        }),
    );
    let http = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (index, value) in [
        torrent_path.to_string_lossy().into_owned(),
        format!("magnet:?xt=urn:btih:{hash}&dn=fixture"),
        format!("http://{http_address}/fixture.torrent"),
    ]
    .into_iter()
    .enumerate()
    {
        let directory = root.path().join(format!("destination-{index}"));
        std::fs::create_dir(&directory)?;
        std::fs::write(directory.join(name), b"existing user bytes")?;
        let record = DownloadSource::parse(&value)?.record(value.clone(), directory.clone(), 2);
        let row = Arc::new(Mutex::new(record));
        let DownloadSource::Bt(source) = DownloadSource::parse(&value)? else {
            anyhow::bail!("BT source missing");
        };
        let revision = AtomicU64::new(0);
        let wake: Wake = Arc::new(|| {});
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            execute(
                source,
                &row,
                &crate::backup::Control::default(),
                &revision,
                &wake,
                &client,
                isolated_options(false),
                vec![address],
            ),
        )
        .await??;
        assert_eq!(std::fs::read(&result)?, bytes);
        assert_eq!(std::fs::read(directory.join(name))?, b"existing user bytes");
        assert_ne!(result, directory.join(name));
        assert_eq!(row.lock().unwrap().downloaded_bytes, bytes.len() as u64);
        assert_eq!(row.lock().unwrap().actual_threads, 0);
        assert!(revision.load(Ordering::Relaxed) >= 2);
    }
    http.abort();
    seed_session.stop().await;
    Ok(())
}
#[tokio::test]
async fn cancelled_bt_task_releases_owner_and_reuses_cached_metadata() -> Result<()> {
    crate::init_crypto();
    let root = tempfile::tempdir()?;
    let name = "resume.bin";
    let bytes = b"verified resumable payload";
    let (metadata, _) = torrent(name, bytes);
    let path = root.path().join("resume.torrent");
    std::fs::write(&path, &metadata)?;
    let row = Arc::new(Mutex::new(Record {
        url: path.display().to_string(),
        protocol: "BT".into(),
        directory: root.path().join("out").display().to_string(),
        ..Default::default()
    }));
    let control = crate::backup::Control::default();
    let cancellation = control.clone();
    let prepared = row.clone();
    let cancellation = tokio::spawn(async move {
        let start = std::time::Instant::now();
        loop {
            if prepared.lock().unwrap().target_file.is_some() {
                cancellation.cancel();
                return Ok::<_, anyhow::Error>(());
            }
            if start.elapsed() > Duration::from_secs(10) {
                cancellation.cancel();
                anyhow::bail!("BT task did not reach prepared state");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let client = reqwest::Client::builder().no_proxy().build()?;
    let revision = AtomicU64::new(0);
    let wake: Wake = Arc::new(|| {});
    let result = tokio::time::timeout(
        Duration::from_secs(12),
        execute(
            Source::File(path.clone()),
            &row,
            &control,
            &revision,
            &wake,
            &client,
            isolated_options(false),
            Vec::new(),
        ),
    )
    .await?;
    cancellation.await??;
    assert!(result.is_err());
    assert!(control.cancelled());
    let record = row.lock().unwrap().clone();
    let lease = target::Target::open(&record).await?;
    assert!(lease.metadata.is_file());
    drop(lease);
    std::fs::remove_file(path)?;
    let seed = root.path().join("seed");
    std::fs::create_dir(&seed)?;
    std::fs::write(seed.join(name), bytes)?;
    let session = Session::new_with_opts(seed, isolated_options(true)).await?;
    let _lifetime = Lifetime(session.cancellation_token().clone());
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(metadata),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("seed missing")?;
    tokio::time::timeout(Duration::from_secs(10), handle.wait_until_completed()).await??;
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        execute(
            Source::File(root.path().join("removed.torrent")),
            &row,
            &crate::backup::Control::default(),
            &revision,
            &wake,
            &client,
            isolated_options(false),
            vec![session.listen_addr().unwrap()],
        ),
    )
    .await??;
    assert_eq!(std::fs::read(result)?, bytes);
    session.stop().await;
    Ok(())
}

#[test]
fn torrent_paths_cannot_escape_or_alias_windows_device_names() {
    for name in [
        "../escape",
        "C:/escape",
        "CON.txt",
        "dir/NUL",
        "trailing.",
        "a:b",
    ] {
        assert!(
            target::validate_relative(std::path::Path::new(name)).is_err(),
            "{name}"
        );
    }
    assert!(target::validate_relative(std::path::Path::new("目录/data.bin")).is_ok());
    assert!(Source::parse("magnet:?dn=missing-hash").is_err());
    assert!(
        Source::parse(
            "magnet:?xt=urn:btih:0123456789012345678901234567890123456789&so=0-18446744073709551615"
        )
        .is_err()
    );
}
