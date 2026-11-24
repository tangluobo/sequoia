//! Task-owned staging and no-clobber publication on the destination volume.
use super::*;
use std::path::{Component, Path};
pub(super) struct Target {
    pub stage: PathBuf,
    pub metadata: PathBuf,
    directory: PathBuf,
    _lease: std::fs::File,
}
pub(super) fn validate_relative(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "种子文件名为空");
    for part in path.components() {
        let Component::Normal(part) = part else {
            anyhow::bail!("种子包含目录外的文件路径");
        };
        let name = part.to_str().context("种子文件名不是 UTF-8")?;
        ensure!(
            crate::terminal::ssh::zmodem::safe_name(name)
                && !name.contains(['<', '>', '"', '|', '?', '*'])
                && !name.chars().any(char::is_control),
            "种子包含无效文件名"
        );
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        ensure!(
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit()),
            "种子包含 Windows 保留文件名"
        );
    }
    Ok(())
}
impl Target {
    pub async fn open(record: &Record) -> Result<Self> {
        let record = record.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Seek, SeekFrom, Write};
            ensure!(
                record.id.len() <= 128
                    && !record.id.is_empty()
                    && record
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "BT 任务标识无效"
            );
            let directory = PathBuf::from(&record.directory);
            std::fs::create_dir_all(&directory)?;
            let directory = directory.canonicalize()?;
            let root = directory.join(format!(".tomato-bt-{}", record.id));
            match std::fs::create_dir(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let meta = std::fs::symlink_metadata(&root)?;
                    ensure!(
                        meta.is_dir() && !meta.is_symlink(),
                        "BT 暂存目录是文件或符号链接"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            let owner = root.join("owner");
            if owner.exists() {
                ensure!(
                    !std::fs::symlink_metadata(&owner)?.is_symlink(),
                    "BT 任务锁是符号链接"
                );
            }
            let mut lease = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(owner)?;
            lease.try_lock().context("该 BT 任务正由另一个下载器使用")?;
            let expected = format!("{}\n{:x}", record.id, Sha1::digest(record.url.as_bytes()));
            let mut stored = String::new();
            (&mut lease).take(1024).read_to_string(&mut stored)?;
            if stored.is_empty() {
                lease.seek(SeekFrom::Start(0))?;
                lease.write_all(expected.as_bytes())?;
                lease.sync_all()?;
            } else {
                ensure!(stored == expected, "BT 任务暂存目录不属于当前下载");
            }
            let stage = root.join("data");
            std::fs::create_dir_all(&stage)?;
            ensure!(
                !std::fs::symlink_metadata(&stage)?.is_symlink(),
                "BT 数据目录是符号链接"
            );
            Ok(Self {
                stage,
                metadata: root.join("source.torrent"),
                directory,
                _lease: lease,
            })
        })
        .await?
    }
    pub fn commit(
        self,
        payload: &Path,
        name: &str,
        selected: Option<&std::collections::BTreeSet<PathBuf>>,
        control: &crate::backup::Control,
    ) -> Result<PathBuf> {
        ensure!(payload.starts_with(&self.stage), "BT 发布路径超出任务目录");
        let meta = std::fs::symlink_metadata(payload)?;
        ensure!(!meta.is_symlink(), "BT 输出不能为符号链接");
        let name = sanitize_name(name);
        validate_relative(Path::new(&name))?;
        for number in 0..10000 {
            let candidate = if number == 0 {
                name.clone()
            } else {
                let path = Path::new(&name);
                let stem = path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("download");
                let suffix = path
                    .extension()
                    .and_then(|v| v.to_str())
                    .map(|v| format!(".{v}"))
                    .unwrap_or_default();
                format!("{stem} ({number}){suffix}")
            };
            let destination = self.directory.join(candidate);
            let reserve = if meta.is_dir() {
                std::fs::create_dir(&destination)
            } else {
                link_or_copy(payload, &destination, control)
            };
            match reserve {
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
                Ok(()) if !meta.is_dir() => {
                    std::fs::remove_file(payload)?;
                    return Ok(destination);
                }
                Ok(()) => {}
            }
            let mut created_files = Vec::new();
            let mut created_dirs = vec![destination.clone()];
            let result = (|| -> Result<()> {
                let mut pending = vec![(payload.to_owned(), destination.clone(), 0)];
                let mut count = 0;
                while let Some((source, target, depth)) = pending.pop() {
                    ensure!(depth <= 64, "BT 输出目录超过 64 层");
                    for entry in std::fs::read_dir(source)? {
                        ensure!(!control.cancelled(), "已取消");
                        let entry = entry?;
                        count += 1;
                        ensure!(count <= 100_000, "BT 输出超过 100000 项");
                        let meta = entry.file_type()?;
                        ensure!(!meta.is_symlink(), "BT 输出不能包含符号链接");
                        if let Some(selected) = selected {
                            let relative = entry.path().strip_prefix(payload)?.to_owned();
                            let keep = if meta.is_dir() {
                                selected
                                    .range(relative.clone()..)
                                    .next()
                                    .is_some_and(|path| path.starts_with(&relative))
                            } else {
                                selected.contains(&relative)
                            };
                            if !keep {
                                continue;
                            }
                        }
                        let output = target.join(entry.file_name());
                        if meta.is_dir() {
                            std::fs::create_dir(&output)?;
                            created_dirs.push(output.clone());
                            pending.push((entry.path(), output, depth + 1));
                        } else {
                            ensure!(meta.is_file(), "BT 输出仅支持普通文件和目录");
                            link_or_copy(&entry.path(), &output, control)?;
                            created_files.push(output);
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                for path in created_files.iter().rev() {
                    let _ = std::fs::remove_file(path);
                }
                for path in created_dirs.iter().rev() {
                    let _ = std::fs::remove_dir(path);
                }
                return Err(error);
            }
            // The final files now own the same verified disk extents; retire
            // only the staging paths corresponding to files published above.
            for path in &created_files {
                if let Ok(relative) = path.strip_prefix(&destination) {
                    let _ = std::fs::remove_file(payload.join(relative));
                }
            }
            for path in created_dirs.iter().rev() {
                if let Ok(relative) = path.strip_prefix(&destination) {
                    let _ = std::fs::remove_dir(payload.join(relative));
                }
            }
            return Ok(destination);
        }
        anyhow::bail!("无法为 BT 下载分配唯一文件名")
    }
}

fn link_or_copy(
    source: &Path,
    target: &Path,
    control: &crate::backup::Control,
) -> std::io::Result<()> {
    use std::io::{Read as _, Write as _};
    match std::fs::hard_link(source, target) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Err(error),
        Err(_) => {}
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let result = (|| {
        let mut buffer = vec![0; 64 * 1024];
        loop {
            if control.cancelled() {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
        }
        output.sync_all()
    })();
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}
