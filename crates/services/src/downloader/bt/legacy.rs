//! Copy legacy partial data read-only before rqbit verifies and resumes its pieces.
use super::*;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
pub(super) async fn seed(
    target: &target::Target,
    record: &Record,
    folder: &std::path::Path,
    payload: &std::path::Path,
    files: &[(PathBuf, u64)],
) -> Result<()> {
    let Some(original) = &record.target_file else {
        return Ok(());
    };
    let original = PathBuf::from(original);
    if !original.try_exists()? || original.starts_with(&target.stage) {
        return Ok(());
    }
    let original = tokio::fs::canonicalize(original).await?;
    let directory = tokio::fs::canonicalize(&record.directory).await?;
    ensure!(
        original.starts_with(&directory),
        "旧 BT 数据路径不属于任务下载目录"
    );
    let root_meta = tokio::fs::metadata(&original).await?;
    for (relative, expected) in files {
        let destination = folder.join(relative);
        ensure!(
            destination.starts_with(&target.stage),
            "BT 续传路径超出任务目录"
        );
        if destination.try_exists()? {
            continue;
        }
        let source = if root_meta.is_file() {
            ensure!(
                files.len() == 1 && destination == payload,
                "旧 BT 文件与种子结构不匹配"
            );
            original.clone()
        } else {
            let relative = destination.strip_prefix(payload)?;
            original.join(relative)
        };
        if !source.try_exists()? {
            continue;
        }
        let source = tokio::fs::canonicalize(source).await?;
        ensure!(
            source == original || source.starts_with(&original),
            "旧 BT 文件超出任务目录"
        );
        let mut reader = tokio::fs::File::open(&source).await?;
        let before = reader.metadata().await?;
        ensure!(before.is_file(), "旧 BT 数据不是普通文件");
        let parent = destination.parent().context("BT 续传目录无效")?;
        tokio::fs::create_dir_all(parent).await?;
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        let mut writer = tokio::fs::File::from_std(temporary.reopen()?);
        let mut buffer = vec![0; 64 * 1024];
        let mut remaining = before.len().min(*expected);
        while remaining > 0 {
            let length = buffer
                .len()
                .min(usize::try_from(remaining).unwrap_or(usize::MAX));
            let count = reader.read(&mut buffer[..length]).await?;
            ensure!(count > 0, "读取旧 BT 数据时文件被缩短");
            writer.write_all(&buffer[..count]).await?;
            remaining -= count as u64;
        }
        writer.flush().await?;
        drop(writer);
        let after = reader.metadata().await?;
        ensure!(
            before.len() == after.len() && before.modified().ok() == after.modified().ok(),
            "复制续传数据时旧 BT 文件被修改"
        );
        temporary
            .persist_noclobber(destination)
            .map_err(|error| error.error)?;
    }
    Ok(())
}
