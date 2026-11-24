use super::*;
use tokio::io::AsyncWriteExt as _;

#[tokio::test]
async fn first_committed_chunk_is_readable_before_download_finishes_and_errors_wake_readers()
-> Result<()> {
    let mut writer = Writer::new("/分块流.txt").await?;
    let asset = writer.asset();
    let directory = asset.directory().to_owned();
    writer.write_all(&vec![b'a'; 64 * 1024]).await?;
    writer.flush().await?;
    let first = asset.clone();
    let read = tokio::task::spawn_blocking(move || -> Result<_> {
        let count = first.stream.available(0, 64 * 1024, || false)?;
        Ok((count, std::fs::read(first.path())?))
    });
    let (count, bytes) = tokio::time::timeout(std::time::Duration::from_secs(2), read).await???;
    assert_eq!(count, 64 * 1024);
    assert_eq!(bytes, vec![b'a'; 64 * 1024]);
    let waiting = asset.clone();
    let read =
        tokio::task::spawn_blocking(move || waiting.stream.available(64 * 1024, 1, || false));
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(!read.is_finished(), "a partial file is not EOF");
    drop(writer);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), read)
            .await??
            .is_err()
    );
    drop(asset);
    assert!(!directory.exists());
    Ok(())
}

#[tokio::test]
async fn completed_stream_has_exact_eof_and_queued_tail_wait_can_be_cancelled() -> Result<()> {
    let mut writer = Writer::new("/completed.txt").await?;
    let asset = writer.asset();
    writer.write_all(b"first").await?;
    writer.flush().await?;
    let cancelled = asset.clone();
    assert!(
        tokio::task::spawn_blocking(move || cancelled.stream.available(5, 1, || true))
            .await?
            .is_err()
    );
    writer.write_all(b"second").await?;
    let finished = writer.finish().await?;
    assert!(Arc::ptr_eq(&asset, &finished));
    assert_eq!(asset.stream.available(0, 64, || false)?, 11);
    assert_eq!(asset.stream.available(11, 64, || false)?, 0);
    assert_eq!(std::fs::read(asset.path())?, b"firstsecond");
    let writer = Writer::new("/empty.txt").await?;
    let empty = writer.finish().await?;
    assert_eq!(empty.stream.available(0, 1, || false)?, 0);
    Ok(())
}
