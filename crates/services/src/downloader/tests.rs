use super::*;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Response, StatusCode},
    routing::get,
};
use std::sync::atomic::AtomicUsize;
#[tokio::test]
async fn reopening_waits_for_a_closing_owner_but_never_overwrites_a_live_owner() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("downloads.json");
    let original = br#"{"version":1,"tasks":[],"future":{"keep":42}}"#;
    std::fs::write(&path, original)?;
    let lock_path = path.with_extension("json.rust-lock");
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lease.try_lock()?;
    let manager = Manager::open(path.clone(), || {});
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(manager.snapshot().error.is_none());
    assert_eq!(manager.revision(), 0);
    drop(lease);
    wait(&manager, |_| manager.revision() > 0).await?;
    assert!(manager.snapshot().error.is_none());
    let second = Manager::open(path.clone(), || {});
    let rejected = wait(&second, |snapshot| snapshot.error.is_some()).await?;
    assert!(rejected.error.unwrap().contains("另一个 Rust 下载器"));
    assert_eq!(std::fs::read(path)?, original);
    drop(second);
    drop(manager);
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)?;
    let started = std::time::Instant::now();
    while lease.try_lock().is_err() {
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(std::fs::read(root.path().join("downloads.json"))?, original);
    Ok(())
}

#[test]
fn unchanged_sparse_history_is_not_rewritten_but_real_changes_are_backed_up() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("downloads.json");
    let bytes =
        br#"{ "version": 1, "tasks": [{"state":"CANCELLED","futureRow":42}], "future":43 }"#;
    std::fs::write(&path, bytes)?;
    let mut original = Some(bytes.to_vec());
    let mut store: Store = serde_json::from_slice(bytes)?;
    let mut fingerprint = Sha1::digest(serde_json::to_vec_pretty(&store)?).into();
    let entries = vec![Entry {
        record: Arc::new(Mutex::new(store.tasks.remove(0))),
        job: None,
        remove: false,
    }];
    save(&path, &mut original, &mut fingerprint, &mut store, &entries)?;
    assert_eq!(std::fs::read(&path)?, bytes);
    assert!(!path.with_extension("json.rust-backup").exists());
    entries[0].record.lock().unwrap().status = "已取消".into();
    save(&path, &mut original, &mut fingerprint, &mut store, &entries)?;
    assert_eq!(
        std::fs::read(path.with_extension("json.rust-backup"))?,
        bytes
    );
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(saved["tasks"][0]["status"], "已取消");
    assert_eq!(saved["tasks"][0]["futureRow"], 42);
    assert_eq!(saved["future"], 43);
    // A subsequent save must still detect a genuine external edit.
    std::fs::write(&path, br#"{"version":1,"tasks":[],"external":true}"#)?;
    entries[0].record.lock().unwrap().status = "changed".into();
    assert!(save(&path, &mut original, &mut fingerprint, &mut store, &entries).is_err());
    assert!(std::fs::read_to_string(path)?.contains("external"));
    Ok(())
}

async fn wait(manager: &Manager, predicate: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let snapshot = manager.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .context("download actor timed out")
}
#[tokio::test]
async fn parallel_pause_resume_old_records_and_external_write_guard() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = directory.path().join("downloads.json");
    let old = serde_json::json!({"version":1,"future":"retained","tasks":[{"url":"magnet:?xt=urn:btih:fixture","protocol":"BITTORRENT","directory":directory.path(),"requestedThreads":4,"fileName":"old torrent","downloadedBytes":123,"totalBytes":456,"state":"CANCELLED","status":"已取消","futureRow":true}]});
    std::fs::write(&store, serde_json::to_vec(&old)?)?;
    let data = Arc::new(
        (0..6 * 1024 * 1024)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let requests = Arc::new(AtomicUsize::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let router = Router::new().route(
        "/fixture.bin",
        get({
            let data = data.clone();
            let requests = requests.clone();
            let bytes = bytes.clone();
            move |headers: HeaderMap| {
                let (data, requests, bytes) = (data.clone(), requests.clone(), bytes.clone());
                async move {
                    let value = headers
                        .get("range")
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .strip_prefix("bytes=")
                        .unwrap();
                    let (start, end) = value.split_once('-').unwrap();
                    let start = start.parse::<usize>().unwrap();
                    let end = end.parse::<usize>().unwrap();
                    requests.fetch_add(1, Ordering::Relaxed);
                    bytes.fetch_add((end - start + 1) as u64, Ordering::Relaxed);
                    let stream = futures_util::stream::unfold(start, move |position| {
                        let data = data.clone();
                        async move {
                            if position > end {
                                return None;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            let next = (position + 32768).min(end + 1);
                            Some((Ok::<_, std::io::Error>(data[position..next].to_vec()), next))
                        }
                    });
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(
                            "content-range",
                            format!("bytes {start}-{end}/{}", 6 * 1024 * 1024),
                        )
                        .header("content-length", end - start + 1)
                        .header("etag", "\"stable\"")
                        .body(Body::from_stream(stream))
                        .unwrap()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/fixture.bin", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let manager = Manager::open(store.clone(), || {});
    let recovered = wait(&manager, |s| s.rows.len() == 1).await?;
    assert_eq!(recovered.rows[0].state, "CANCELLED");
    assert_eq!(requests.load(Ordering::Relaxed), 0);
    manager.send(Action::Add {
        url,
        directory: directory.path().into(),
        threads: 2,
    })?;
    let running = wait(&manager, |s| {
        s.rows
            .iter()
            .any(|r| r.file_name == "fixture.bin" && r.downloaded_bytes >= 2 * 1024 * 1024)
    })
    .await?;
    let id = running
        .rows
        .iter()
        .find(|r| r.file_name == "fixture.bin")
        .unwrap()
        .id
        .clone();
    manager.send(Action::Cancel(id.clone()))?;
    wait(&manager, |s| {
        s.rows.iter().any(|r| r.id == id && r.state == "CANCELLED")
    })
    .await?;
    assert!(!directory.path().join("fixture.bin").exists());
    manager.send(Action::Start(id.clone()))?;
    let done = wait(&manager, |s| {
        s.rows.iter().any(|r| r.id == id && r.state == "COMPLETED")
    })
    .await?;
    let row = done.rows.iter().find(|r| r.id == id).unwrap();
    assert_eq!(row.downloaded_bytes, data.len() as u64);
    assert_eq!(std::fs::read(row.completed_file.as_ref().unwrap())?, *data);
    assert!(requests.load(Ordering::Relaxed) >= 8);
    assert!(
        bytes.load(Ordering::Relaxed) < 12 * 1024 * 1024,
        "resume must reuse completed chunks"
    );
    // Disk persistence trails the in-memory completion notification by a single actor turn.
    for _ in 0..100 {
        if std::fs::read(&store)
            .ok()
            .is_some_and(|v| String::from_utf8_lossy(&v).contains("COMPLETED"))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&store)?)?;
    assert_eq!(saved["future"], "retained");
    assert_eq!(saved["tasks"][0]["futureRow"], true);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(
            store.with_extension("json.rust-backup")
        )?)?,
        old
    );
    std::fs::write(&store, b"external change")?;
    manager.send(Action::ClearCompleted)?;
    wait(&manager, |s| {
        s.error.as_deref().is_some_and(|e| e.contains("其他程序"))
    })
    .await?;
    assert_eq!(std::fs::read(&store)?, b"external change");
    drop(manager);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn server_ignoring_range_uses_single_stream_and_never_replaces_existing_file() -> Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("plain"), b"original")?;
    let data = vec![42u8; 123456];
    let body = data.clone();
    let router = Router::new().route(
        "/plain",
        get(move || {
            let body = body.clone();
            async move { body }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/plain", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let manager = Manager::open(directory.path().join("downloads.json"), || {});
    manager.send(Action::Add {
        url: url.clone(),
        directory: directory.path().into(),
        threads: 32,
    })?;
    manager.send(Action::Add {
        url,
        directory: directory.path().into(),
        threads: 32,
    })?;
    wait(&manager, |s| {
        s.rows.iter().filter(|r| r.state == "COMPLETED").count() == 2
    })
    .await?;
    assert_eq!(std::fs::read(directory.path().join("plain"))?, b"original");
    assert_eq!(std::fs::read(directory.path().join("plain (1)"))?, data);
    assert_eq!(std::fs::read(directory.path().join("plain (2)"))?, data);
    drop(manager);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn empty_download_uses_utf8_content_disposition_after_range_rejection() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let router=Router::new().route("/opaque",get(|headers:HeaderMap|async move {
        if headers.contains_key("range") {Response::builder().status(StatusCode::RANGE_NOT_SATISFIABLE).body(Body::empty()).unwrap()}
        else {Response::builder().header("content-disposition","attachment; filename=old.bin; filename*=UTF-8''%E7%A9%BA%E6%96%87%E4%BB%B6.txt").body(Body::empty()).unwrap()}
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/opaque", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let manager = Manager::open(directory.path().join("downloads.json"), || {});
    manager.send(Action::Add {
        url,
        directory: directory.path().into(),
        threads: 4,
    })?;
    let completed = wait(&manager, |s| s.rows.iter().any(|r| r.state == "COMPLETED")).await?;
    assert_eq!(completed.rows[0].file_name, "空文件.txt");
    assert_eq!(
        std::fs::metadata(directory.path().join("空文件.txt"))?.len(),
        0
    );
    drop(manager);
    server.abort();
    Ok(())
}
