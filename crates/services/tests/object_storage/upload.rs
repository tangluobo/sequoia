use super::*;
#[path = "native.rs"]
mod native;
use tomato_services::files::object_copy::{Job, Phase};
struct Server {
    state: StateData,
    types: Arc<Mutex<BTreeMap<String, String>>>,
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
    gate: Arc<Gate>,
}
#[derive(Default)]
struct Gate {
    hold: std::sync::atomic::AtomicBool,
    entered: std::sync::atomic::AtomicUsize,
    wake: tokio::sync::Notify,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.gate
            .hold
            .store(false, std::sync::atomic::Ordering::Release);
        self.gate.wake.notify_waiters();
        self.task.abort();
    }
}
type UploadState = (StateData, Arc<Mutex<BTreeMap<String, String>>>, Arc<Gate>);
async fn upload_serve(
    State((data, types, gate)): State<UploadState>,
    request: Request,
) -> Response {
    if request.method() == http::Method::PUT
        && request
            .uri()
            .query()
            .is_some_and(|query| query.contains("partNumber="))
    {
        gate.entered
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        while gate.hold.load(std::sync::atomic::Ordering::Acquire) {
            tokio::select! {_=gate.wake.notified()=>{},_=tokio::time::sleep(std::time::Duration::from_millis(100))=>{}}
        }
    }
    if (request.method() == http::Method::PUT || request.uri().query() == Some("uploads"))
        && let Some(value) = request
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    {
        let path = percent_encoding::percent_decode_str(request.uri().path())
            .decode_utf8_lossy()
            .into_owned();
        types.lock().unwrap().insert(path, value.into());
    }
    serve(State(data), request).await
}
impl Server {
    async fn start() -> Result<Self> {
        let state = StateData::default();
        state.lock().unwrap().buckets.insert("fixture".into());
        let types = Arc::new(Mutex::new(BTreeMap::new()));
        let gate = Arc::new(Gate::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = axum::Router::new().fallback(upload_serve).with_state((
            state.clone(),
            types.clone(),
            gate.clone(),
        ));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            state,
            types,
            address,
            task,
            gate,
        })
    }
    fn connection(&self, kind: &str) -> Connection {
        let mut connection = Connection::new(kind);
        if kind == "LOCAL_DIRECTORY" {
            connection.set("directoryType", "S3");
        }
        connection.set("endpoint", format!("http://{}", self.address));
        connection.set("region", "us-east-1");
        connection.set("username", "fixture-key");
        connection.set("password", "fixture-secret");
        connection.set("pathStyleAccess", true);
        connection
    }
}
#[tokio::test]
async fn file_drop_upload_s3_and_oss_keep_current_prefix_empty_folders_content_type_and_refresh()
-> Result<()> {
    for kind in ["S3", "ALIYUN_OSS", "LOCAL_DIRECTORY"] {
        for fixed in [true, false] {
            let server = Server::start().await?;
            let source = tempfile::tempdir()?;
            let folder = source.path().join("文件夹");
            tokio::fs::create_dir_all(folder.join("空目录")).await?;
            tokio::fs::write(folder.join("index.html"), "<h1>上传测试</h1>").await?;
            let empty = source.path().join("zero.txt");
            tokio::fs::write(&empty, []).await?;
            let mut connection = server.connection(kind);
            if fixed {
                connection.set("bucket", "fixture");
            }
            let storage = Storage::new(&connection)?;
            let directory = if fixed { "/base" } else { "/fixture/base" };
            server
                .state
                .lock()
                .unwrap()
                .files
                .insert(("fixture".into(), "base/zero.txt".into()), b"old".to_vec());
            let Output::Uploaded { report, .. } = storage
                .execute(Operation::UploadFiles {
                    directory: directory.into(),
                    paths: vec![folder, empty],
                    deferred: Vec::new(),
                    job: Arc::new(Job::default()),
                })
                .await?
            else {
                anyhow::bail!("upload report expected")
            };
            assert_eq!(
                (report.total, report.succeeded, report.failed),
                (4, 4, 0),
                "{kind}/fixed={fixed}: {:?}",
                report.errors
            );
            assert_eq!(report.phase, Phase::Finished);
            {
                let objects = server.state.lock().unwrap();
                assert_eq!(
                    objects
                        .files
                        .get(&("fixture".into(), "base/文件夹/index.html".into()))
                        .unwrap(),
                    "<h1>上传测试</h1>".as_bytes()
                );
                assert!(
                    objects
                        .files
                        .contains_key(&("fixture".into(), "base/文件夹/空目录/".into()))
                );
                assert!(
                    !objects
                        .files
                        .contains_key(&("fixture".into(), "base/文件夹/".into())),
                    "nonempty object directories must not get phantom markers"
                );
                assert!(
                    objects
                        .files
                        .get(&("fixture".into(), "base/zero.txt".into()))
                        .unwrap()
                        .is_empty()
                );
            }
            assert_eq!(
                server
                    .types
                    .lock()
                    .unwrap()
                    .get("/fixture/base/文件夹/index.html")
                    .map(String::as_str),
                Some("text/html")
            );
            let Output::Directory { entries, .. } = storage.list(directory).await? else {
                anyhow::bail!("refresh expected")
            };
            assert_eq!(entries.len(), 2);
            if !fixed {
                let Output::Uploaded { report, .. } = storage
                    .execute(Operation::UploadFiles {
                        directory: "/".into(),
                        paths: vec![source.path().join("zero.txt")],
                        deferred: Vec::new(),
                        job: Arc::new(Job::default()),
                    })
                    .await?
                else {
                    anyhow::bail!("upload report expected")
                };
                assert_eq!(report.phase, Phase::Failed);
                assert!(report.current.contains("Bucket"));
            }
        }
    }
    Ok(())
}
