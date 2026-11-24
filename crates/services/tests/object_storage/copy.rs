use super::*;
#[path = "cancellation.rs"]
mod cancellation;
use tomato_services::files::{
    clipboard::{self, Content, Repository as _},
    object_copy::{self, Job, Phase, Selection},
};
#[derive(Default)]
struct Clipboard(Mutex<Option<Content>>);
impl clipboard::Repository for Clipboard {
    fn read(&self) -> Result<Content> {
        self.0
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("empty fixture clipboard"))
    }
    fn write_objects(&self, selection: &Selection) -> Result<()> {
        *self.0.lock().unwrap() = Some(Content::Objects(selection.clone()));
        Ok(())
    }
}
struct Server {
    state: StateData,
    task: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Result<Self> {
        let state = StateData::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = axum::Router::new()
            .fallback(serve)
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            state,
            task,
            address,
        })
    }
    fn connection(&self, kind: &str) -> Connection {
        let mut c = Connection::new(kind);
        c.set("endpoint", format!("http://{}", self.address));
        c.set("region", "us-east-1");
        c.set("username", "fixture-key");
        c.set("password", "fixture-secret");
        c.set("pathStyleAccess", true);
        c
    }
}
#[tokio::test]
async fn original_object_clipboard_reads_encrypted_source_and_server_copies_across_buckets_with_partial_failures()
-> Result<()> {
    let server = Server::start().await?;
    let config = tempfile::tempdir()?;
    let path = config.path().join("connections.json");
    let c = server.connection("S3");
    let store = tomato_core::Store {
        connections: vec![c.clone()],
        ..Default::default()
    };
    store.save(&path)?;
    let original = std::fs::read(&path)?;
    let clip = Arc::new(Clipboard::default());
    let context = object_copy::Context {
        clipboard: clip.clone(),
        connections: Arc::new(object_copy::FileConnections(path.clone())),
    };
    let storage = Storage::new(&c)?.with_copy_context(context);
    {
        let mut state = server.state.lock().unwrap();
        for bucket in ["from", "to"] {
            state.buckets.insert(bucket.into());
        }
        for (key, value) in [
            ("dir/a.txt", "first"),
            ("dir/fail.md", "failed only when requested"),
            ("dir/sub/中文|🍅.md", "nested body"),
        ] {
            state
                .files
                .insert(("from".into(), key.into()), value.as_bytes().to_vec());
        }
        state.files.insert(
            ("to".into(), "dest/dir/a.txt".into()),
            b"old value".to_vec(),
        );
    }
    let Output::ObjectsStaged(notice) = storage
        .execute(Operation::CopyObjects {
            directory: "/from".into(),
            paths: vec![("/from/dir".into(), true)],
        })
        .await?
    else {
        anyhow::bail!("copy must stage the private clipboard")
    };
    assert_eq!(notice, "已复制 0 个文件, 1 个目录 到剪贴板");
    let Content::Objects(selection) = clip.read()? else {
        anyhow::bail!("wrong clipboard type")
    };
    assert_eq!(selection.bucket, "from");
    assert_eq!(selection.items[0].key, "dir/");
    let job = Arc::new(Job::default());
    let Output::ObjectsCopied(report) = storage
        .execute(Operation::PasteObjects {
            directory: "/to/dest".into(),
            job,
        })
        .await?
    else {
        anyhow::bail!("paste did not complete")
    };
    assert_eq!((report.total, report.succeeded, report.failed), (3, 3, 0));
    assert_eq!(report.phase, Phase::Finished);
    {
        let state = server.state.lock().unwrap();
        assert_eq!(
            state.files[&("to".into(), "dest/dir/a.txt".into())],
            b"first"
        );
        assert_eq!(
            state.files[&("to".into(), "dest/dir/sub/中文|🍅.md".into())],
            b"nested body"
        );
        assert_eq!(state.copy_requests, 3);
        assert_eq!(
            state
                .files
                .keys()
                .filter(|(bucket, _)| bucket == "from")
                .count(),
            3
        );
    }
    server.state.lock().unwrap().fail_copy = true;
    let Output::ObjectsCopied(report) = storage
        .execute(Operation::PasteObjects {
            directory: "/to/partial".into(),
            job: Arc::new(Job::default()),
        })
        .await?
    else {
        anyhow::bail!("missing partial report")
    };
    assert_eq!((report.succeeded, report.failed), (2, 1));
    assert!(report.errors[0].0.contains("403"));
    assert!(
        server
            .state
            .lock()
            .unwrap()
            .files
            .contains_key(&("to".into(), "partial/dir/sub/中文|🍅.md".into()))
    );
    server.state.lock().unwrap().fail_copy = false;
    server.state.lock().unwrap().embedded_copy_error = true;
    let Output::ObjectsCopied(embedded) = storage
        .execute(Operation::PasteObjects {
            directory: "/to/embedded".into(),
            job: Arc::new(Job::default()),
        })
        .await?
    else {
        anyhow::bail!("missing embedded failure report")
    };
    assert_eq!((embedded.succeeded, embedded.failed), (2, 1));
    assert!(
        embedded.errors[0].0.contains("fixture embedded copy error"),
        "HTTP 200 with an Error body must not count as success"
    );
    server.state.lock().unwrap().embedded_copy_error = false;
    let Output::ObjectsCopied(report) = storage
        .execute(Operation::PasteObjects {
            directory: "/from/dir/child".into(),
            job: Arc::new(Job::default()),
        })
        .await?
    else {
        anyhow::bail!("missing child-directory copy")
    };
    assert_eq!(
        report.total, 3,
        "enumeration must finish before new child keys are written"
    );
    assert!(
        storage
            .execute(Operation::CopyObjects {
                directory: "/".into(),
                paths: vec![("/from".into(), true)]
            })
            .await
            .is_err()
    );
    let signed = server.state.lock().unwrap().signed;
    assert!(
        storage
            .execute(Operation::PasteObjects {
                directory: "/".into(),
                job: Arc::new(Job::default())
            })
            .await
            .is_err()
    );
    assert_eq!(
        server.state.lock().unwrap().signed,
        signed,
        "root paste must not send requests"
    );
    assert_eq!(
        std::fs::read(path)?,
        original,
        "copy never rewrites legacy configuration"
    );
    Ok(())
}

#[tokio::test]
async fn cross_connection_s3_and_oss_copy_streams_complete_binary_and_preserves_legacy_prefix()
-> Result<()> {
    for (source_kind, target_kind) in [
        ("S3", "ALIYUN_OSS"),
        ("ALIYUN_OSS", "S3"),
        ("S3", "S3"),
        ("ALIYUN_OSS", "ALIYUN_OSS"),
    ] {
        let server = Server::start().await?;
        let mut source = server.connection(source_kind);
        source.set("bucket", "from");
        source.set("s3Prefix", "workspace/");
        let mut target = server.connection(target_kind);
        target.set("bucket", "to");
        target.set("s3Prefix", "destination/");
        let binary: Vec<u8> = (0..1_048_607).map(|i| (i % 251) as u8).collect();
        {
            let mut state = server.state.lock().unwrap();
            state.buckets.extend(["from".into(), "to".into()]);
            state
                .files
                .insert(("from".into(), "workspace/中文.bin".into()), binary.clone());
        }
        let clip = Arc::new(Clipboard::default());
        struct Lookup(Connection);
        impl object_copy::Connections for Lookup {
            fn find(&self, id: &str) -> Result<Connection> {
                anyhow::ensure!(id == self.0.id, "wrong source id");
                Ok(self.0.clone())
            }
        }
        let context = object_copy::Context {
            clipboard: clip.clone(),
            connections: Arc::new(Lookup(source.clone())),
        };
        let source_storage = Storage::new(&source)?.with_copy_context(context.clone());
        let target_storage = Storage::new(&target)?.with_copy_context(context);
        source_storage
            .execute(Operation::CopyObjects {
                directory: "/".into(),
                paths: vec![("/中文.bin".into(), false)],
            })
            .await?;
        let Content::Objects(selected) = clip.read()? else {
            anyhow::bail!("missing staged source")
        };
        assert_eq!(selected.items[0].key, "workspace/中文.bin");
        let Output::ObjectsCopied(report) = target_storage
            .execute(Operation::PasteObjects {
                directory: "/nested".into(),
                job: Arc::new(Job::default()),
            })
            .await?
        else {
            anyhow::bail!("missing cross-provider result")
        };
        assert_eq!(
            (report.succeeded, report.failed),
            (1, 0),
            "{source_kind} -> {target_kind}: {:?}",
            report.errors
        );
        let state = server.state.lock().unwrap();
        assert_eq!(
            state.files[&("to".into(), "destination/nested/中文.bin".into())],
            binary
        );
        assert_eq!(
            state.files[&("from".into(), "workspace/中文.bin".into())],
            binary
        );
        assert_eq!(
            state.copy_requests, 0,
            "different connections use download/upload"
        );
        assert!(state.uploads.is_empty());
    }
    Ok(())
}
