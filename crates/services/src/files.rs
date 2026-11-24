pub mod clipboard;
pub mod document;
pub mod export;
pub mod images;
pub mod object_copy;
pub mod object_tree;
pub mod operations;
pub mod session;
pub mod storage;
pub mod tree;
pub mod upload;
use anyhow::{Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub directory: bool,
    pub bytes: u64,
}
impl Entry {
    /// Includes a conservative allowance for the listing Vec's spare capacity.
    pub fn retained_bytes(&self) -> usize {
        2 * std::mem::size_of::<Self>() + self.name.capacity() + self.path.capacity()
    }
}

pub fn list(path: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        entries.push(Entry {
            name: entry.file_name().to_string_lossy().into(),
            path: entry.path(),
            directory: metadata.is_dir(),
            bytes: metadata.len(),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// The connection tree follows Java File.isHidden and loads one level at a time.
/// Reject oversized listings instead of retaining an unbounded tree in the UI.
pub fn list_visible(path: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut retained = 0usize;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        #[cfg(windows)]
        let hidden = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 2 != 0
        };
        #[cfg(not(windows))]
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if hidden {
            continue;
        }
        anyhow::ensure!(
            entries.len() < 5000,
            "连接树单个目录最多加载 5,000 项，请缩小浏览目录范围"
        );
        let value = Entry {
            name: entry.file_name().to_string_lossy().into(),
            path: entry.path(),
            directory: if metadata.file_type().is_symlink() {
                entry.path().is_dir()
            } else {
                metadata.is_dir()
            },
            bytes: metadata.len(),
        };
        retained = retained.saturating_add(value.retained_bytes());
        anyhow::ensure!(
            retained <= 8 * 1024 * 1024,
            "目录树列表超过 8 MiB 内存预算，请缩小浏览范围"
        );
        entries.push(value);
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn create_directory(parent: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
    {
        bail!("请输入单一目录名");
    }
    let path = parent.join(name);
    std::fs::create_dir(&path)?;
    Ok(path)
}

pub fn save_text(path: &Path, content: &str) -> Result<()> {
    tomato_core::config::atomic_write(path, content.as_bytes())
}

pub fn export_csv(path: &Path, columns: &[String], rows: &[Vec<String>]) -> Result<()> {
    let escape = |cell: &str| format!("\"{}\"", cell.replace('"', "\"\""));
    let mut result = String::from("\u{feff}");
    for row in std::iter::once(columns).chain(rows.iter().map(Vec::as_slice)) {
        result.push_str(
            &row.iter()
                .map(|cell| escape(cell))
                .collect::<Vec<_>>()
                .join(","),
        );
        result.push_str("\r\n");
    }
    save_text(path, &result)
}

/// Original table import format: UTF-8, one row per line, tab-separated, no header.
/// It only stages values; the user must explicitly save them through the workbench.
pub fn import_table_rows(
    path: &Path,
    definition: &tomato_core::workbench::TableDefinition,
    remaining: usize,
) -> Result<Vec<Vec<Option<tomato_core::workbench::Cell>>>> {
    use std::io::Read;
    use tomato_core::{limits, workbench::Cell};
    if definition.view || !definition.transactional {
        bail!("当前对象不可写入");
    }
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(limits::QUERY_BYTES as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > limits::QUERY_BYTES {
        bail!("每次最多导入 8 MiB 数据");
    }
    let mut rows = Vec::new();
    let mut retained = 0usize;
    for line in text
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|line| !line.is_empty())
    {
        if rows.len() >= remaining.min(1000) {
            bail!("暂存新行最多 1,000 行，请分批导入并保存");
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() > definition.columns.len() {
            bail!("第 {} 行字段数超过表结构", rows.len() + 1);
        }
        let row = definition
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let value = fields.get(index).copied().unwrap_or_default();
                if column.generated || (column.auto_increment && value.is_empty()) {
                    return Ok(None);
                }
                let cell = if column.binary {
                    if !value.len().is_multiple_of(2) {
                        bail!("二进制列 {} 需要偶数长度的十六进制", column.name);
                    }
                    let bytes = value
                        .as_bytes()
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
                        .collect::<Result<Vec<_>>>()?;
                    Cell::Binary(bytes)
                } else {
                    Cell::Text(value.into())
                };
                Ok(Some(cell))
            })
            .collect::<Result<Vec<_>>>()?;
        retained =
            retained.saturating_add(row.len() * std::mem::size_of::<Option<Cell>>() + line.len());
        if retained > limits::QUERY_BYTES {
            bail!("导入数据超过 8 MiB 暂存上限");
        }
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tree_tests {
    use super::*;
    #[test]
    fn tree_listing_keeps_original_directory_order_and_platform_hidden_semantics() -> Result<()> {
        let directory = tempfile::tempdir()?;
        std::fs::create_dir(directory.path().join("Zfolder"))?;
        std::fs::write(directory.path().join("B.md"), "")?;
        std::fs::write(directory.path().join("a.txt"), "")?;
        let hidden = directory.path().join(".hidden");
        std::fs::write(&hidden, "")?;
        #[cfg(windows)]
        anyhow::ensure!(
            std::process::Command::new("attrib")
                .arg("+H")
                .arg(&hidden)
                .status()?
                .success(),
            "failed to mark fixture hidden"
        );
        assert_eq!(
            list_visible(directory.path())?
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Zfolder", "a.txt", "B.md"]
        );
        assert!(list_visible(&directory.path().join("missing")).is_err());
        Ok(())
    }
    #[test]
    fn tree_listing_rejects_oversized_directories_instead_of_retaining_all_entries() -> Result<()> {
        let directory = tempfile::tempdir()?;
        for index in 0..5001 {
            std::fs::File::create(directory.path().join(index.to_string()))?;
        }
        assert!(list_visible(directory.path()).is_err());
        Ok(())
    }
}
