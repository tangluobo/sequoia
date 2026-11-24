use super::*;
use tomato_services::files::{
    operations::{Operation as FileOperation, Output as FileOutput},
    storage::Storage,
};

#[tokio::test]
async fn s3_and_oss_keep_signed_host_through_ssh_for_buckets_and_objects() -> Result<()> {
    for kind in ["S3", "ALIYUN_OSS"] {
        let jump = Fixture::start(Mode::Shell).await?;
        let listener =
            tokio::net::TcpListener::bind(if kind == "S3" || !cfg!(target_os = "windows") {
                "127.0.0.1:0"
            } else {
                "127.0.0.2:0"
            })
            .await?;
        let address = listener.local_addr()?;
        let authority = address.to_string();
        let body = Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = axum::Router::new().fallback({
            let body = body.clone();
            let requests = requests.clone();
            move |request: axum::extract::Request| {
                let body = body.clone();
                let requests = requests.clone();
                let authority = authority.clone();
                async move {
                    use axum::body::{Body, to_bytes};
                    // Socket destination changed to the tunnel port; both the
                    // signed authority and actual HTTP Host must remain original.
                    assert_eq!(request.headers()[http::header::HOST], authority);
                    assert!(request.headers()[http::header::AUTHORIZATION].to_str().unwrap().contains("fixture-key"));
                    requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let path = request.uri().path().to_owned();
                    let method = request.method().clone();
                    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
                    let bytes = if path == "/" {
                        b"<ListAllMyBucketsResult><Buckets><Bucket><Name>fixture</Name></Bucket></Buckets></ListAllMyBucketsResult>".to_vec()
                    } else if path.ends_with("/object.txt") && method == http::Method::PUT {
                        *body.lock().unwrap() = bytes.to_vec();
                        Vec::new()
                    } else if method == http::Method::GET || method == http::Method::HEAD {
                        body.lock().unwrap().clone()
                    } else { Vec::new() };
                    http::Response::builder().status(200)
                        .header("content-length", bytes.len())
                        .header("etag", "\"fixture\"")
                        .body(Body::from(if method == http::Method::HEAD { Vec::new() } else { bytes })).unwrap()
                }
            }
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut gateway = Connection::new("SSH");
        gateway.set("host", "127.0.0.1");
        gateway.set("port", jump.port);
        gateway.set("username", "fixture");
        gateway.set("password", "fixture-only");
        let mut connection = Connection::new(kind);
        connection.set("endpoint", format!("http://{address}"));
        connection.set("username", "fixture-key");
        connection.set("password", "fixture-secret");
        connection.set("pathStyleAccess", true);
        connection.set("useSshTunnel", true);
        let before = serde_json::to_value(&connection)?;
        let storage =
            Storage::connect_via(&connection, &gateway, jump.root.path().join("known_hosts"))
                .await?;
        storage.check().await?;
        let FileOutput::Directory { entries, .. } = storage.list("/").await? else {
            anyhow::bail!("Missing buckets");
        };
        assert_eq!(entries[0].name, "fixture");
        storage
            .execute(FileOperation::WriteText {
                path: "/fixture/object.txt".into(),
                text: "原生 S3/OSS SSH 数据".into(),
            })
            .await?;
        let FileOutput::Text { text, .. } = storage
            .execute(FileOperation::ReadText("/fixture/object.txt".into()))
            .await?
        else {
            anyhow::bail!("Missing text");
        };
        assert_eq!(text, "原生 S3/OSS SSH 数据");
        assert_eq!(serde_json::to_value(&connection)?, before);
        assert!(requests.load(std::sync::atomic::Ordering::Relaxed) >= 4);
        drop(storage);
        server.abort();
    }
    Ok(())
}
