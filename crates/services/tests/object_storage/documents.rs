use super::*;
use tomato_services::{
    commands::Output as CommandOutput,
    files::{document::Action, export, session::FileSession},
    terminal::Status,
};
#[tokio::test]
async fn s3_and_oss_live_documents_and_drag_export_handle_prefix_and_all_bucket_roots() -> Result<()>
{
    let state = StateData::default();
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("docs".into());
        objects.files.insert(
            ("docs".into(), "prefix/文档.md".into()),
            b"# original".to_vec(),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = axum::Router::new()
        .fallback(serve)
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    for kind in ["S3", "ALIYUN_OSS"] {
        for all_buckets in [false, true] {
            let mut connection = Connection::new(kind);
            for (key, value) in [
                ("username", "fixture-key"),
                ("password", "fixture-secret"),
                ("region", "us-east-1"),
            ] {
                connection.set(key, value);
            }
            connection.set("endpoint", format!("http://{address}"));
            connection.set("pathStyleAccess", true);
            let path = if all_buckets {
                "/docs/prefix/文档.md"
            } else {
                connection.set("bucket", "docs");
                connection.set("s3Prefix", "prefix/");
                "/文档.md"
            };
            let mut session = FileSession::start(&tokio::runtime::Handle::current(), connection);
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while session.status() == Status::Connecting {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await?;
            assert_eq!(session.status(), Status::Ready);
            let client = session.document_client();
            assert!(
                matches!(client.execute(Action::Read(path.into())).await?,CommandOutput::Text(text)if text.starts_with('#'))
            );
            client
                .execute(Action::Write {
                    path: path.into(),
                    text: format!("# {kind} saved 中文"),
                })
                .await?;
            let Output::ExportReady(actual, asset) = client
                .request(Operation::ExportFile(export::Request::new(path.into())))
                .await?
            else {
                anyhow::bail!("expected export")
            };
            // An all-buckets adapter normalizes its displayed path inside the child bucket.
            assert!(actual.ends_with("文档.md"));
            assert_eq!(
                tokio::fs::read_to_string(asset.path()).await?,
                format!("# {kind} saved 中文")
            );
            assert_eq!(
                state.lock().unwrap().files[&("docs".into(), "prefix/文档.md".into())],
                format!("# {kind} saved 中文").as_bytes()
            );
            session.stop().await;
            assert!(client.execute(Action::Read(path.into())).await.is_err());
        }
    }
    Ok(())
}
