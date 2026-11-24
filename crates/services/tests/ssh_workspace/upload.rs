use super::*;
#[tokio::test]
async fn file_drop_upload_preserves_sftp_tree_and_continues_after_one_missing_file() -> Result<()> {
    use tomato_services::files::object_copy::{Job, Phase};
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SFTP").await?;
    request(&mut terminal, Operation::Mkdir("/target".into())).await?;
    let source = tempfile::tempdir()?;
    let folder = source.path().join("文件夹");
    tokio::fs::create_dir_all(folder.join("空目录")).await?;
    let bytes: Vec<_> = (0..270_000).map(|n| (n % 251) as u8).collect();
    tokio::fs::write(folder.join("中文.bin"), &bytes).await?;
    let staged_root = Arc::new(OnceLock::new());
    let staged = staged_root.clone();
    let deferred = tomato_services::files::upload::Deferred::new(
        64,
        Arc::new(move || {
            let stage = || -> Result<_> {
                let lease = tempfile::tempdir()?;
                let path = lease.path().join("ZIP 中文.txt");
                std::fs::write(&path, "archive content")?;
                staged
                    .set(lease.path().to_path_buf())
                    .expect("upload staged once");
                Ok((vec![path], Box::new(lease) as Box<dyn Send>))
            };
            stage().map_err(|error| error.to_string())
        }),
    );
    let Output::Uploaded { report, .. } = request(
        &mut terminal,
        Operation::UploadFiles {
            directory: "/target".into(),
            paths: vec![source.path().join("missing.txt"), folder],
            deferred: vec![deferred],
            job: Arc::new(Job::default()),
        },
    )
    .await?
    else {
        anyhow::bail!("upload report expected")
    };
    assert_eq!((report.total, report.succeeded, report.failed), (5, 4, 1));
    assert_eq!(report.phase, Phase::Finished);
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("target/ZIP 中文.txt"))?,
        "archive content"
    );
    assert!(
        !staged_root
            .get()
            .context("archive entries were not staged")?
            .exists()
    );
    assert!(fixture.root.path().join("target/文件夹/空目录").is_dir());
    assert_eq!(
        tokio::fs::read(fixture.root.path().join("target/文件夹/中文.bin")).await?,
        bytes
    );
    let Output::Directory { entries, .. } =
        request(&mut terminal, Operation::List("/target/文件夹".into())).await?
    else {
        anyhow::bail!("refresh expected")
    };
    assert_eq!(entries.len(), 2);
    Ok(())
}
