use super::*;
use std::sync::Arc;
use tomato_services::files::object_copy::{Job, Phase};
#[tokio::test]
async fn file_drop_upload_to_native_ftp_streams_unicode_tree_and_overwrites() -> Result<()> {
    use tomato_core::server::{ServerAccount, ServerConfig, ServerType};
    let target = tempfile::tempdir()?;
    let source = tempfile::tempdir()?;
    let config = ServerConfig {
        kind: ServerType::Ftp,
        port: 0,
        bind_address: "127.0.0.1".into(),
        root_directory: remote(target.path()),
        accounts: vec![ServerAccount {
            username: "fixture".into(),
            password: "fixture-only".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let server = tomato_services::servers::ftp::start(config).await?;
    let mut connection = Connection::new("FTP");
    connection.set("host", "127.0.0.1");
    connection.set("port", server.address.port());
    connection.set("username", "fixture");
    connection.set("password", "fixture-only");
    let folder = source.path().join("文件夹");
    tokio::fs::create_dir_all(folder.join("空目录")).await?;
    let data: Vec<_> = (0..270_000).map(|n| (n % 251) as u8).collect();
    tokio::fs::write(folder.join("中文.bin"), &data).await?;
    let storage = Storage::new(&connection)?;
    storage.check().await?;
    for _ in 0..2 {
        let Output::Uploaded { report, .. } = storage
            .execute(Operation::UploadFiles {
                directory: "/".into(),
                paths: vec![folder.clone()],
                deferred: Vec::new(),
                job: Arc::new(Job::default()),
            })
            .await?
        else {
            anyhow::bail!("upload report expected")
        };
        assert_eq!(
            (report.total, report.succeeded, report.failed),
            (3, 3, 0),
            "{:?}",
            report.errors
        );
    }
    assert!(target.path().join("文件夹/空目录").is_dir());
    assert_eq!(
        tokio::fs::read(target.path().join("文件夹/中文.bin")).await?,
        data
    );
    drop(server);
    Ok(())
}

#[tokio::test]
async fn file_drop_upload_preserves_tree_empty_files_overwrite_and_partial_failure_in_real_actor()
-> Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let folder = source.path().join("文件夹");
    tokio::fs::create_dir_all(folder.join("空目录")).await?;
    tokio::fs::create_dir_all(folder.join("子目录")).await?;
    let data: Vec<_> = (0..270_000).map(|n| (n % 251) as u8).collect();
    tokio::fs::write(folder.join("子目录/中文.bin"), &data).await?;
    let empty = source.path().join("zero.txt");
    tokio::fs::write(&empty, []).await?;
    tokio::fs::write(target.path().join("zero.txt"), b"old content").await?;
    let runtime = tokio::runtime::Handle::current();
    let mut session = FileSession::start(&runtime, connection(target.path()));
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.status() != Status::Ready {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    let job = Arc::new(Job::default());
    let output = request(
        &mut session,
        Operation::UploadFiles {
            directory: remote(target.path()),
            paths: vec![folder, empty, source.path().join("missing.txt")],
            deferred: Vec::new(),
            job: job.clone(),
        },
    )
    .await?;
    let Output::Uploaded { report, .. } = output else {
        anyhow::bail!("upload report expected")
    };
    assert_eq!((report.total, report.succeeded, report.failed), (6, 5, 1));
    assert_eq!(report.phase, Phase::Finished);
    assert_eq!(report.total_bytes, Some(data.len() as u64));
    assert_eq!(report.completed_bytes, data.len() as u64);
    assert!(target.path().join("文件夹/空目录").is_dir());
    assert_eq!(
        tokio::fs::read(target.path().join("文件夹/子目录/中文.bin")).await?,
        data
    );
    assert!(
        tokio::fs::read(target.path().join("zero.txt"))
            .await?
            .is_empty()
    );
    let Output::Directory { entries, .. } =
        request(&mut session, Operation::List(remote(target.path()))).await?
    else {
        anyhow::bail!("refresh expected")
    };
    assert_eq!(entries.len(), 2);
    session.stop().await;
    Ok(())
}

#[tokio::test]
async fn file_drop_upload_cancels_an_active_stream_without_retaining_the_file_in_memory()
-> Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let path = source.path().join("stream.bin");
    let file = std::fs::File::create(&path)?;
    file.set_len(128 * 1024 * 1024)?;
    drop(file);
    let storage = Storage::new(&connection(target.path()))?;
    let job = Arc::new(Job::default());
    let worker = job.clone();
    let directory = remote(target.path());
    let task = tokio::spawn(async move {
        storage
            .execute(Operation::UploadFiles {
                directory,
                paths: vec![path],
                deferred: Vec::new(),
                job: worker,
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let report = job.snapshot();
            if report.transferred > 0 {
                break;
            }
            assert!(
                !matches!(report.phase, Phase::Finished | Phase::Failed),
                "stream ended before cancellation"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await?;
    job.cancel();
    let Output::Uploaded { report, .. } =
        tokio::time::timeout(Duration::from_secs(5), task).await???
    else {
        anyhow::bail!("upload report expected")
    };
    assert_eq!(report.phase, Phase::Cancelled);
    assert_eq!(report.succeeded, 0);
    tomato_services::files::storage::wait_upload_cleanup().await;
    Ok(())
}
