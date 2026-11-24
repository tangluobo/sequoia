//! Independent TCP fixtures: packet fields below follow the Java/eMule wire layout.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tomato_ed2k::{md4::md4, types::ED2K_CHUNK_SIZE};

struct Fixture {
    server: std::net::SocketAddr,
    payload: Arc<Vec<u8>>,
    hash: [u8; 16],
    allow_tail: tokio::sync::watch::Sender<bool>,
    sent: Arc<AtomicU64>,
    active: Arc<AtomicUsize>,
    large_request: Arc<AtomicBool>,
    reject_repeated_hashsets: Arc<AtomicBool>,
    failures: Arc<Mutex<Vec<String>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn read(stream: &mut TcpStream) -> Result<(u8, u8, Vec<u8>)> {
    let mut header = [0; 6];
    stream.read_exact(&mut header).await?;
    let len = u32::from_le_bytes(header[1..5].try_into()?) as usize;
    ensure!((1..=2_000_000).contains(&len), "fixture packet length");
    let mut body = vec![0; len - 1];
    stream.read_exact(&mut body).await?;
    Ok((header[0], header[5], body))
}
async fn send(stream: &mut TcpStream, protocol: u8, opcode: u8, body: &[u8]) -> Result<()> {
    let mut packet = vec![protocol];
    packet.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
    packet.push(opcode);
    packet.extend_from_slice(body);
    // Split headers and payloads to exercise streaming assembly, not one read/packet.
    for chunk in packet.chunks(16381) {
        stream.write_all(chunk).await?;
    }
    Ok(())
}

impl Fixture {
    async fn open(payload: Vec<u8>, pause_tail: bool, large_metadata: bool) -> Result<Self> {
        let payload = Arc::new(payload);
        let hashes: Vec<_> = payload.chunks(ED2K_CHUNK_SIZE as usize).map(md4).collect();
        let hash = if hashes.len() == 1 {
            hashes[0]
        } else {
            md4(&hashes.iter().flatten().copied().collect::<Vec<_>>())
        };
        let size = if large_metadata {
            5_311_711_232
        } else {
            payload.len() as u64
        };
        let peer = TcpListener::bind("127.0.0.1:0").await?;
        let peer_port = peer.local_addr()?.port();
        let server_listener = TcpListener::bind("127.0.0.1:0").await?;
        let server = server_listener.local_addr()?;
        let (allow_tail, tail) = tokio::sync::watch::channel(!pause_tail);
        let sent = Arc::new(AtomicU64::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let failures = Arc::new(Mutex::new(Vec::new()));
        let large_request = Arc::new(AtomicBool::new(false));
        let reject_repeated_hashsets = Arc::new(AtomicBool::new(false));
        let mut tasks = Vec::new();
        {
            let active = active.clone();
            let failures = failures.clone();
            tasks.push(tokio::spawn(async move {
                let mut clients = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        _ = clients.join_next(), if !clients.is_empty() => {},
                        accepted = server_listener.accept() => {
                            let Ok((mut stream, _)) = accepted else { break };
                            let active = active.clone(); let failures = failures.clone();
                            clients.spawn(async move {
                                active.fetch_add(1, Ordering::SeqCst); let _active = Active(active);
                                let result: Result<()> = async {
                                    let (proto, opcode, hello) = read(&mut stream).await?;
                                    ensure!(proto == 0xe3 && opcode == 1 && hello.len() >= 26, "server login");
                                    send(&mut stream, 0xe3, 0x40, &u32::from_le_bytes([127, 0, 0, 1]).to_le_bytes()).await?;
                                    loop {
                                        let (_, op, data) = read(&mut stream).await?;
                                        if op == 0x19 {
                                            ensure!(data.get(..16) == Some(hash.as_slice()), "source request hash");
                                            let requested = if size > u32::MAX as u64 {
                                                ensure!(data.len() == 28 && data[16..20] == [0; 4], "large file source marker");
                                                u64::from_le_bytes(data[20..28].try_into()?)
                                            } else { ensure!(data.len() == 20, "source size"); u32::from_le_bytes(data[16..20].try_into()?) as u64 };
                                            ensure!(requested == size, "source request truncated file size");
                                            let mut sources = hash.to_vec(); sources.push(1);
                                            sources.extend_from_slice(&[127, 0, 0, 1]); sources.extend_from_slice(&peer_port.to_le_bytes());
                                            send(&mut stream, 0xe3, 0x42, &sources).await?;
                                        }
                                    }
                                }.await;
                                if let Err(e) = result && !e.downcast_ref::<std::io::Error>().is_some_and(|e| matches!(e.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe)) { failures.lock().unwrap().push(e.to_string()); }
                            });
                        }
                    }
                }
            }));
        }
        {
            let payload = payload.clone();
            let sent = sent.clone();
            let active = active.clone();
            let failures = failures.clone();
            let large_request = large_request.clone();
            let reject_repeated_hashsets = reject_repeated_hashsets.clone();
            tasks.push(tokio::spawn(async move {
                let mut clients = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        _ = clients.join_next(), if !clients.is_empty() => {},
                        accepted = peer.accept() => {
                            let Ok((mut stream, _)) = accepted else { break };
                            let payload = payload.clone(); let sent = sent.clone(); let active = active.clone();
                            let failures = failures.clone(); let hashes = hashes.clone(); let mut tail = tail.clone();
                            let large_request = large_request.clone();
                            let reject_repeated_hashsets = reject_repeated_hashsets.clone();
                            clients.spawn(async move {
                                active.fetch_add(1, Ordering::SeqCst); let _active = Active(active);
                                let result: Result<()> = async {
                                    stream.set_nodelay(true)?;
                                    let (protocol, opcode, hello) = read(&mut stream).await?;
                                    ensure!(protocol == 0xe3 && opcode == 1 && hello.first() == Some(&16), "peer hello");
                                    send(&mut stream, 0xe3, 0x4c, &hello[1..]).await?;
                                    loop {
                                        let (proto, op, data) = read(&mut stream).await?;
                                        match (proto, op) {
                                            (0xe3, 0x4f) => { let mut status = hash.to_vec(); status.extend_from_slice(&0u16.to_le_bytes()); send(&mut stream, 0xe3, 0x50, &status).await?; }
                                            (0xe3, 0x51) => { ensure!(!reject_repeated_hashsets.load(Ordering::SeqCst),"peer rejects redundant cached hashset requests"); if large_metadata { continue; } let mut body = hash.to_vec(); body.extend_from_slice(&(hashes.len() as u16).to_le_bytes()); for h in &hashes { body.extend_from_slice(h); } send(&mut stream, 0xe3, 0x52, &body).await?; }
                                            (0xe3, 0x54) => { send(&mut stream, 0xc5, 0x60, &[2, 0]).await?; tokio::time::sleep(Duration::from_millis(10)).await; send(&mut stream, 0xe3, 0x55, &[]).await?; }
                                            (0xe3, 0x47) | (0xc5, 0xa3) => {
                                                let wide = op == 0xa3; let width = if wide { 8 } else { 4 };
                                                ensure!(data.len() == 16 + 6 * width, "request parts layout");
                                                ensure!(data[..16] == hash, "part file hash");
                                                ensure!(wide == large_metadata, "wrong 32/64-bit block request");
                                                if wide { large_request.store(true, Ordering::SeqCst); }
                                                if large_metadata { continue; }
                                                for i in 0..3 {
                                                    let pos = |field: usize| -> u64 { let at = 16 + (field * 3 + i) * width; if wide { u64::from_le_bytes(data[at..at+8].try_into().unwrap()) } else { u32::from_le_bytes(data[at..at+4].try_into().unwrap()) as u64 } };
                                                    let (start, end) = (pos(0), pos(1));
                                                    if start == end { continue; }
                                                    ensure!(end > start && end <= payload.len() as u64 && end-start <= 184_320, "requested block bounds");
                                                    if start >= ED2K_CHUNK_SIZE { while !*tail.borrow_and_update() { tail.changed().await?; } }
                                                    for (j, chunk) in payload[start as usize..end as usize].chunks(32768).enumerate() {
                                                        let offset = start + (j * 32768) as u64;
                                                        let mut block = hash.to_vec(); block.extend_from_slice(&(offset as u32).to_le_bytes()); block.extend_from_slice(&((offset + chunk.len() as u64) as u32).to_le_bytes()); block.extend_from_slice(chunk);
                                                        send(&mut stream, 0xe3, 0x46, &block).await?;
                                                        sent.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }.await;
                                if let Err(e) = result && !e.downcast_ref::<std::io::Error>().is_some_and(|e| matches!(e.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe)) { failures.lock().unwrap().push(e.to_string()); }
                            });
                        }
                    }
                }
            }));
        }
        Ok(Self {
            server,
            payload,
            hash,
            allow_tail,
            sent,
            active,
            large_request,
            reject_repeated_hashsets,
            failures,
            tasks,
        })
    }
    fn configure(&self, profile: &Path) -> Result<()> {
        std::fs::create_dir_all(profile.join("rust-ed2k"))?;
        std::fs::write(
            profile.join("rust-ed2k/network.json"),
            serde_json::to_vec(
                &serde_json::json!({ "servers": [self.server.to_string()], "kad": false, "loopbackOnly": true }),
            )?,
        )?;
        Ok(())
    }
    fn link(&self, size: u64) -> String {
        let hash: String = self.hash.iter().map(|v| format!("{v:02X}")).collect();
        format!("ed2k://|file|fixture.bin|{size}|{hash}|/")
    }
}

async fn wait(manager: &Manager, predicate: impl Fn(&Record) -> bool) -> Result<Record> {
    let start = std::time::Instant::now();
    loop {
        let snapshot = manager.snapshot();
        ensure!(
            snapshot.error.is_none(),
            "actor error: {:?}",
            snapshot.error
        );
        if let Some(row) = snapshot
            .rows
            .iter()
            .find(|r| r.protocol == "ED2K" && predicate(r))
        {
            return Ok(row.clone());
        }
        ensure!(
            start.elapsed() < Duration::from_secs(25),
            "eD2k fixture timeout: {:?}",
            snapshot.rows
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_ed2k_actor_discovers_downloads_verifies_resumes_and_releases_sockets() -> Result<()>
{
    let directory = tempfile::tempdir()?;
    let bytes: Vec<_> = (0..ED2K_CHUNK_SIZE as usize + 262_145)
        .map(|i| (i.wrapping_mul(17) % 251) as u8)
        .collect();
    let fixture = Fixture::open(bytes, true, false).await?;
    fixture.configure(directory.path())?;
    let history = directory.path().join("downloads.json");
    std::fs::write(
        &history,
        serde_json::to_vec(
            &serde_json::json!({"version":1,"tasks":[{"url":"magnet:?xt=urn:btih:fixture","protocol":"BITTORRENT","state":"CANCELLED","rustId":"legacy","future":42}],"future":43}),
        )?,
    )?;
    let manager = Manager::open(history.clone(), || {});
    manager.send(Action::Add {
        url: fixture.link(fixture.payload.len() as u64),
        directory: directory.path().join("files"),
        threads: 1,
    })?;
    let row = wait(&manager, |r| {
        r.downloaded_bytes >= ED2K_CHUNK_SIZE && r.state == "DOWNLOADING"
    })
    .await?;
    let part = tomato_ed2k::partial::part_path(Path::new(row.target_file.as_deref().unwrap()));
    manager.send(Action::Cancel(row.id.clone()))?;
    wait(&manager, |r| r.state == "CANCELLED").await?;
    let verified = std::fs::read(tomato_ed2k::partial::met_path(Path::new(
        row.target_file.as_deref().unwrap(),
    )))?;
    let verified: serde_json::Value = serde_json::from_slice(&verified)?;
    assert_eq!(verified["verified"], serde_json::json!([0]));
    assert!(part.exists());
    drop(manager);
    fixture.allow_tail.send(true)?;
    let manager = Manager::open(history.clone(), || {});
    manager.send(Action::Start(row.id.clone()))?;
    let row = wait(&manager, |r| r.state == "COMPLETED").await?;
    assert_eq!(row.downloaded_bytes, fixture.payload.len() as u64);
    assert_eq!(
        std::fs::read(row.completed_file.as_ref().unwrap())?,
        *fixture.payload
    );
    assert!(!part.exists());
    assert!(
        fixture.sent.load(Ordering::SeqCst) < fixture.payload.len() as u64 + 500_000,
        "verified first part was downloaded again"
    );
    drop(manager);
    let start = std::time::Instant::now();
    loop {
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(history.with_extension("json.rust-lock"))?;
        if lease.try_lock().is_ok() && fixture.active.load(Ordering::SeqCst) == 0 {
            break;
        }
        ensure!(
            start.elapsed() < Duration::from_secs(4),
            "actor/socket shutdown timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&history)?)?;
    assert_eq!(saved["future"], 43);
    assert_eq!(saved["tasks"][0]["future"], 42);
    assert!(history.with_extension("json.rust-backup").exists());
    assert!(
        fixture.failures.lock().unwrap().is_empty(),
        "{:?}",
        fixture.failures.lock().unwrap()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_java_history_restarts_the_native_engine_and_reuses_verified_windows_image_parts()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let payload: Vec<_> = (0..ED2K_CHUNK_SIZE as usize + 196_608)
        .map(|index| (index % 251) as u8)
        .collect();
    let fixture = Fixture::open(payload, true, false).await?;
    fixture.configure(directory.path())?;
    let history = directory.path().join("downloads.json");
    let manager = Manager::open(history.clone(), || {});
    manager.send(Action::Add {
        url: fixture.link(fixture.payload.len() as u64),
        directory: directory.path().join("files"),
        threads: 1,
    })?;
    let old = wait(&manager, |row| {
        row.downloaded_bytes >= ED2K_CHUNK_SIZE && row.state == "DOWNLOADING"
    })
    .await?;
    manager.send(Action::Cancel(old.id.clone()))?;
    wait(&manager, |row| row.state == "CANCELLED").await?;
    drop(manager);
    let started = std::time::Instant::now();
    loop {
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(history.with_extension("json.rust-lock"))?;
        if lease.try_lock().is_ok() {
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "actor did not release legacy history"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&history)?)?;
    let row = saved["tasks"][0].as_object_mut().unwrap();
    row.remove("rustId");
    row.insert("protocol".into(), "HTTP".into());
    row.insert("state".into(), "FAILED".into());
    row.insert("status".into(), "invalid domain character".into());
    row.insert(
        "legacyExtension".into(),
        serde_json::json!({"preserved":true}),
    );
    std::fs::write(&history, serde_json::to_vec(&saved)?)?;
    fixture
        .reject_repeated_hashsets
        .store(true, Ordering::SeqCst);
    fixture.allow_tail.send(true)?;
    let manager = Manager::open(history.clone(), || {});
    // The legacy label is deliberately HTTP until Start dispatches by the URI.
    // The ordinary fixture waiter filters ED2K rows and cannot observe this state.
    let restored = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = manager.snapshot();
            ensure!(snapshot.error.is_none(), "{:?}", snapshot.error);
            if let Some(row) = snapshot.rows.first() {
                break Ok::<_, anyhow::Error>(row.clone());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    ensure!(
        restored.state == "FAILED" && restored.protocol == "HTTP",
        "legacy labels should survive the read-only load"
    );
    ensure!(
        restored.id != old.id,
        "legacy task must receive a new runtime id"
    );
    manager.send(Action::Start(restored.id))?;
    let completed = wait(&manager, |row| row.state == "COMPLETED").await?;
    ensure!(
        completed.protocol == "ED2K" && completed.target_file == old.target_file,
        "restart must preserve the selected destination and use the native engine"
    );
    ensure!(
        std::fs::read(completed.completed_file.unwrap())? == *fixture.payload,
        "resumed image bytes differ"
    );
    ensure!(
        fixture.sent.load(Ordering::SeqCst) < fixture.payload.len() as u64 + 500_000,
        "verified parts were fetched again"
    );
    ensure!(
        completed.extra["legacyExtension"]["preserved"] == true,
        "legacy fields lost"
    );
    let failures = fixture.failures.lock().unwrap();
    ensure!(failures.is_empty(), "{failures:?}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_4gib_links_use_real_64_bit_source_and_peer_requests_without_preallocating()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let fixture = Fixture::open(b"metadata only".to_vec(), false, true).await?;
    fixture.configure(dir.path())?;
    let manager = Manager::open(dir.path().join("downloads.json"), || {});
    manager.send(Action::Add {
        url: fixture.link(5_311_711_232),
        directory: dir.path().join("files"),
        threads: 1,
    })?;
    let start = std::time::Instant::now();
    while !fixture.large_request.load(Ordering::SeqCst) {
        ensure!(
            start.elapsed() < Duration::from_secs(10),
            "large file wire request missing: {:?}",
            manager.snapshot().rows
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let row = wait(&manager, |r| r.state == "DOWNLOADING").await?;
    assert_eq!(row.total_bytes, 5_311_711_232);
    assert_eq!(
        std::fs::metadata(tomato_ed2k::partial::part_path(Path::new(
            row.target_file.as_deref().unwrap()
        )))?
        .len(),
        0
    );
    manager.send(Action::Cancel(row.id))?;
    wait(&manager, |r| r.state == "CANCELLED").await?;
    assert!(fixture.failures.lock().unwrap().is_empty());
    Ok(())
}
