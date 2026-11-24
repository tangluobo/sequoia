use super::Snapshot;
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tomato_core::game_dungeons::Board;

const LIMIT: usize = 16 * 1024 * 1024;
pub(super) struct FileRepository {
    path: PathBuf,
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("无法读取副本记录"),
    };
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "副本记录超过 16 MiB，原文件未修改");
    Ok(Some(bytes))
}
impl FileRepository {
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub(super) fn load(&self) -> Result<Snapshot> {
        let Some(bytes) = bytes(&self.path)? else {
            return Ok(Snapshot {
                board: Board::default(),
                revision: None,
            });
        };
        let board: Board =
            serde_json::from_slice(&bytes).context("副本记录格式无效，原文件未修改")?;
        board.validate()?;
        Ok(Snapshot {
            board,
            revision: Some(digest(&bytes)),
        })
    }
    pub(super) fn save(&self, board: &Board, revision: Option<&str>) -> Result<String> {
        board.validate()?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut lock_path = self.path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(PathBuf::from(lock_path))?;
        lock.try_lock()
            .context("副本记录正被其他程序保存，请稍后重试")?;
        let original = bytes(&self.path)?;
        ensure!(
            original.as_deref().map(digest).as_deref() == revision,
            "副本记录已被外部修改，请刷新后重试；本次未覆盖外部修改"
        );
        let encoded = serde_json::to_vec_pretty(board)?;
        ensure!(encoded.len() <= LIMIT, "副本记录超过 16 MiB");
        if let Some(original) = &original {
            let mut backup = self.path.as_os_str().to_os_string();
            backup.push(".before-rust.bak");
            let backup = PathBuf::from(backup);
            if !backup.exists() {
                let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
                temporary.write_all(original)?;
                temporary.as_file().sync_all()?;
                temporary.persist_noclobber(backup).map_err(|e| e.error)?;
            }
        }
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&encoded)?;
        temporary.as_file().sync_all()?;
        ensure!(
            bytes(&self.path)? == original,
            "副本记录已被外部修改，保存已取消"
        );
        temporary.persist(&self.path).map_err(|e| e.error)?;
        Ok(digest(&encoded))
    }
}
