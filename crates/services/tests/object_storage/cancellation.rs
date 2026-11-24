use super::*;
#[path = "native_capture.rs"]
mod native_capture;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tomato_services::files::{
    object_copy::{Item, Owner, Request},
    session::FileSession,
};
#[derive(Default)]
struct Pause {
    entered: AtomicUsize,
    release: Notify,
}
async fn paused(
    axum::extract::State((data, pause)): axum::extract::State<(StateData, Arc<Pause>)>,
    request: axum::extract::Request,
) -> axum::response::Response {
    if request.method() == http::Method::PUT
        && request
            .uri()
            .query()
            .is_some_and(|q| q.contains("partNumber="))
    {
        pause.entered.fetch_add(1, Ordering::Release);
        pause.release.notified().await;
    }
    serve(axum::extract::State(data), request).await
}
async fn fixture() -> Result<(Server, Arc<Pause>, Connection, Connection, Selection)> {
    let state = StateData::default();
    {
        let mut s = state.lock().unwrap();
        s.buckets.extend(["from".into(), "to".into()]);
        s.files.insert(
            ("from".into(), "large.bin".into()),
            vec![0xa5; 7 * 1024 * 1024],
        );
    }
    let pause = Arc::new(Pause::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = axum::Router::new()
        .fallback(paused)
        .with_state((state.clone(), pause.clone()));
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let server = Server {
        state,
        task,
        address,
    };
    let mut source = server.connection("S3");
    source.set("bucket", "from");
    let mut target = server.connection("ALIYUN_OSS");
    target.set("bucket", "to");
    let selection = Selection {
        connection_id: source.id.clone(),
        kind: source.kind().into(),
        bucket: "from".into(),
        items: vec![Item {
            key: "large.bin".into(),
            directory: false,
        }],
    };
    Ok((server, pause, source, target, selection))
}
async fn until(predicate: impl Fn() -> bool) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(20);
    while !predicate() {
        anyhow::ensure!(Instant::now() < end, "copy fixture timed out");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Ok(())
}
fn request(
    source: &Connection,
    target: &Connection,
    selection: &Selection,
    prefix: &str,
) -> Request {
    Request {
        source: source.clone(),
        target: target.clone(),
        selection: selection.clone(),
        bucket: "to".into(),
        prefix: prefix.into(),
    }
}
#[tokio::test]
async fn cancelling_active_upload_aborts_native_multipart_and_keeps_source() -> Result<()> {
    let (server, pause, source, target, selection) = fixture().await?;
    let job = Arc::new(Job::default());
    let owner = Owner(job.clone());
    let request = request(&source, &target, &selection, "cancelled");
    let worker_job = job.clone();
    let task = tokio::spawn(async move { object_copy::execute(request, worker_job).await });
    until(|| pause.entered.load(Ordering::Acquire) > 0).await?;
    assert!(!server.state.lock().unwrap().uploads.is_empty());
    drop(owner);
    let report = tokio::time::timeout(Duration::from_secs(7), task).await???;
    pause.release.notify_waiters();
    assert_eq!(report.phase, Phase::Cancelled);
    assert_eq!((report.succeeded, report.failed), (0, 0));
    let state = server.state.lock().unwrap();
    assert!(
        state.uploads.is_empty(),
        "cancel must send AbortMultipartUpload"
    );
    assert!(
        !state
            .files
            .contains_key(&("to".into(), "cancelled/large.bin".into()))
    );
    assert_eq!(
        state.files[&("from".into(), "large.bin".into())].len(),
        7 * 1024 * 1024
    );
    Ok(())
}
#[tokio::test]
async fn closing_file_actor_cleans_inflight_multipart_even_when_write_future_is_aborted()
-> Result<()> {
    let (server, pause, source, target, selection) = fixture().await?;
    let clipboard = Arc::new(Clipboard::default());
    *clipboard.0.lock().unwrap() = Some(Content::Objects(selection));
    struct Lookup(Connection);
    impl object_copy::Connections for Lookup {
        fn find(&self, id: &str) -> Result<Connection> {
            anyhow::ensure!(id == self.0.id, "wrong source");
            Ok(self.0.clone())
        }
    }
    let context = object_copy::Context {
        clipboard,
        connections: Arc::new(Lookup(source)),
    };
    let mut actor =
        FileSession::start_with_context(&tokio::runtime::Handle::current(), target, context);
    until(|| actor.status() == tomato_services::terminal::Status::Ready).await?;
    let job = Arc::new(Job::default());
    let owner = Owner(job.clone());
    actor.request(Operation::PasteObjects {
        directory: "/closed".into(),
        job: job.clone(),
    })?;
    until(|| pause.entered.load(Ordering::Acquire) > 0).await?;
    drop(owner);
    drop(actor);
    until(|| server.state.lock().unwrap().uploads.is_empty()).await?;
    pause.release.notify_waiters();
    assert!(job.is_cancelled());
    assert!(
        !server
            .state
            .lock()
            .unwrap()
            .files
            .contains_key(&("to".into(), "closed/large.bin".into()))
    );
    Ok(())
}
#[tokio::test]
async fn two_copy_limit_allows_waiting_job_to_cancel_before_other_transfers_finish() -> Result<()> {
    let (server, pause, source, target, selection) = fixture().await?;
    let mut workers = Vec::new();
    let mut owners = Vec::new();
    for index in 0..2 {
        let job = Arc::new(Job::default());
        owners.push(Owner(job.clone()));
        let request = request(&source, &target, &selection, &format!("active-{index}"));
        workers.push(tokio::spawn(async move {
            object_copy::execute(request, job).await
        }));
    }
    until(|| pause.entered.load(Ordering::Acquire) == 2).await?;
    let third = Arc::new(Job::default());
    let owner = Owner(third.clone());
    let request = request(&source, &target, &selection, "never-written");
    let worker_job = third.clone();
    let pending = tokio::spawn(async move { object_copy::execute(request, worker_job).await });
    until(|| third.snapshot().phase == Phase::Waiting).await?;
    drop(owner);
    let report = tokio::time::timeout(Duration::from_secs(2), pending).await???;
    assert_eq!(report.phase, Phase::Cancelled);
    assert_eq!(report.total, 0);
    drop(owners);
    pause.release.notify_waiters();
    for worker in workers {
        let report = tokio::time::timeout(Duration::from_secs(20), worker).await???;
        assert_eq!(report.phase, Phase::Cancelled);
    }
    let state = server.state.lock().unwrap();
    assert!(state.uploads.is_empty());
    assert_eq!(state.files.len(), 1);
    Ok(())
}

#[tokio::test]
async fn s3_keeps_empty_directory_marker_but_original_oss_recursive_copy_skips_it() -> Result<()> {
    for kind in ["S3", "ALIYUN_OSS"] {
        let server = Server::start().await?;
        {
            let mut state = server.state.lock().unwrap();
            state.buckets.extend(["from".into(), "to".into()]);
            state
                .files
                .insert(("from".into(), "empty/".into()), Vec::new());
        }
        let mut source = server.connection(kind);
        source.set("bucket", "from");
        let mut target = source.clone();
        target.set("bucket", "to");
        let selection = Selection {
            connection_id: source.id.clone(),
            kind: kind.into(),
            bucket: "from".into(),
            items: vec![Item {
                key: "empty/".into(),
                directory: true,
            }],
        };
        let report = object_copy::execute(
            request(&source, &target, &selection, "copied"),
            Arc::new(Job::default()),
        )
        .await?;
        assert_eq!(report.failed, 0);
        assert_eq!(report.succeeded, usize::from(kind == "S3"));
        assert_eq!(
            server
                .state
                .lock()
                .unwrap()
                .files
                .contains_key(&("to".into(), "copied/empty/".into())),
            kind == "S3"
        );
    }
    Ok(())
}
#[tokio::test]
async fn stopping_plain_upload_actor_waits_for_abort_before_the_runtime_is_dropped() -> Result<()> {
    let (server, pause, _, target, _) = fixture().await?;
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("upload.bin");
    tokio::fs::write(&file, vec![0x5a; 7 * 1024 * 1024]).await?;
    let mut actor = FileSession::start(&tokio::runtime::Handle::current(), target);
    until(|| actor.status() == tomato_services::terminal::Status::Ready).await?;
    actor.request(Operation::Upload {
        local: file.clone(),
        remote: "/ordinary.bin".into(),
    })?;
    until(|| pause.entered.load(Ordering::Acquire) > 0).await?;
    actor.stop().await;
    tokio::time::timeout(
        Duration::from_secs(7),
        tomato_services::files::storage::wait_upload_cleanup(),
    )
    .await?;
    actor.stop().await;
    assert_eq!(actor.status(), tomato_services::terminal::Status::Exited);
    assert!(server.state.lock().unwrap().uploads.is_empty());
    assert!(
        !server
            .state
            .lock()
            .unwrap()
            .files
            .contains_key(&("to".into(), "ordinary.bin".into()))
    );
    assert_eq!(tokio::fs::metadata(file).await?.len(), 7 * 1024 * 1024);
    pause.release.notify_waiters();
    Ok(())
}
