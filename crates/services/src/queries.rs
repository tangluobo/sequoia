//! Named SQL query repository using the original .tomato directory hierarchy.
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
};
use tomato_core::queries::{Entry, Output, Request, Scope};
static WRITES: Mutex<()> = Mutex::new(());
const MAX_TEXT: u64 = 8 * 1024 * 1024;
pub struct Repository {
    root: PathBuf,
}
impl Repository {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn execute(&self, request: Request) -> Result<Output> {
        ensure!(self.root.is_absolute(), "查询数据目录需要绝对路径");
        let _write = if matches!(&request, Request::List(_) | Request::Read { .. }) {
            None
        } else {
            Some(
                WRITES
                    .lock()
                    .map_err(|_| anyhow::anyhow!("查询文件写入锁失效"))?,
            )
        };
        let scope = request.scope().clone();
        let directory = self.root.join(scope.directory()?);
        match request {
            Request::EnsureDirectory(_) => {
                self.prepare(&directory)?;
                Ok(Output::Directory {
                    scope,
                    path: directory,
                })
            }
            Request::List(_) => {
                let mut entries = Vec::new();
                if !directory.exists() {
                    return Ok(Output::Listed { scope, entries });
                }
                self.inside(&directory)?;
                let mut files = Vec::new();
                let mut bytes = 0usize;
                for entry in std::fs::read_dir(&directory)? {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name = name.to_str().context("查询文件名不是有效 Unicode")?;
                    let directory = entry.path().is_dir();
                    let name = if directory {
                        name
                    } else {
                        let Some(name) = name.strip_suffix(".sql") else {
                            continue;
                        };
                        name
                    };
                    bytes = bytes.saturating_add(name.len() + std::mem::size_of::<Entry>());
                    ensure!(
                        entries.len() + files.len() < 10_000 && bytes <= 8 * 1024 * 1024,
                        "查询列表超过 10000 项或 8 MiB"
                    );
                    let item = Entry {
                        name: name.into(),
                        directory,
                    };
                    if directory {
                        entries.push(item);
                    } else {
                        files.push(item);
                    }
                }
                entries.extend(files);
                Ok(Output::Listed { scope, entries })
            }
            Request::Read { name, .. } => {
                let file = self.root.join(scope.file(&name)?);
                self.inside(&file)?;
                let bytes = read(&file)?;
                let stamp = stamp(&bytes);
                let text = String::from_utf8(bytes).context("SQL 查询文件不是 UTF-8")?;
                Ok(Output::Document {
                    scope,
                    name,
                    text,
                    stamp,
                })
            }
            Request::Save {
                name,
                text,
                expected,
                ..
            } => {
                ensure!(text.len() as u64 <= MAX_TEXT, "SQL 查询超过 8 MiB");
                self.prepare(&directory)?;
                let file = self.root.join(scope.file(&name)?);
                self.inside(&file)?;
                let before = read_optional(&file)?;
                let before_stamp = before.as_deref().map(stamp);
                if let Some(expected) = expected {
                    ensure!(
                        before_stamp.as_deref() == Some(expected.as_str()),
                        "查询已被外部修改或删除，保留当前草稿，请重新加载后再保存"
                    );
                }
                if let Some(bytes) = &before {
                    atomic(&file.with_extension("sql.bak"), bytes)?;
                }
                let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
                temporary.write_all(text.as_bytes())?;
                temporary.as_file().sync_all()?;
                drop(before);
                ensure!(
                    file_stamp(&file)? == before_stamp,
                    "保存期间查询文件被外部修改，保留当前草稿"
                );
                temporary.persist(&file).map_err(|error| error.error)?;
                Ok(Output::Saved {
                    scope,
                    name,
                    stamp: stamp(text.as_bytes()),
                })
            }
            Request::CreateDirectory { name, .. } => {
                self.prepare(&directory)?;
                let child = self.root.join(scope.child(&name)?.directory()?);
                self.inside(&child)?;
                ensure!(!child.exists(), "查询目录已存在");
                std::fs::create_dir(&child)?;
                Ok(Output::Changed {
                    scope,
                    renamed: None,
                    deleted: Vec::new(),
                    errors: Vec::new(),
                })
            }
            Request::Rename { entry, name, .. } => {
                let old = self.entry(&scope, &entry)?;
                let new = self.entry(
                    &scope,
                    &Entry {
                        name,
                        directory: entry.directory,
                    },
                )?;
                self.inside(&old)?;
                self.inside(&new)?;
                ensure!(old.exists(), "查询或目录已不存在");
                ensure!(
                    old.canonicalize()? != self.root.join(scope.root()?).canonicalize()?,
                    "不能重命名查询根目录"
                );
                ensure!(old.is_dir() == entry.directory, "查询项目类型已改变");
                ensure!(old != new && !new.exists(), "目标名称已存在或与原名称相同");
                std::fs::rename(&old, &new)?;
                Ok(Output::Changed {
                    scope,
                    renamed: Some((old, new)),
                    deleted: Vec::new(),
                    errors: Vec::new(),
                })
            }
            Request::Delete { entries, .. } => {
                ensure!(entries.len() <= 1000, "一次最多删除 1000 个查询项目");
                let paths = entries
                    .iter()
                    .map(|entry| self.entry(&scope, entry))
                    .collect::<Result<Vec<_>>>()?;
                for (path, entry) in paths.iter().zip(&entries) {
                    self.inside(path)?;
                    ensure!(path != &self.root.join(scope.root()?), "不能删除查询根目录");
                    if path.exists() {
                        ensure!(
                            path.canonicalize()? != self.root.join(scope.root()?).canonicalize()?,
                            "不能删除查询根目录"
                        );
                        ensure!(path.is_dir() == entry.directory, "查询项目类型已改变");
                    }
                }
                let mut deleted = Vec::new();
                let mut errors = Vec::new();
                for (path, entry) in paths.into_iter().zip(entries) {
                    let result = if path.exists() {
                        if entry.directory {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        }
                    } else {
                        Ok(())
                    };
                    match result {
                        Ok(()) => deleted.push(path),
                        Err(error) => errors.push(format!("{}: {error}", entry.name)),
                    }
                }
                Ok(Output::Changed {
                    scope,
                    renamed: None,
                    deleted,
                    errors,
                })
            }
        }
    }
    fn entry(&self, scope: &Scope, entry: &Entry) -> Result<PathBuf> {
        Ok(self.root.join(if entry.directory {
            scope.child(&entry.name)?.directory()?
        } else {
            scope.file(&entry.name)?
        }))
    }
    fn prepare(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        self.inside(path)?;
        std::fs::create_dir_all(path)?;
        self.inside(path)
    }
    fn inside(&self, path: &Path) -> Result<()> {
        ensure!(
            path.is_absolute()
                && path.starts_with(&self.root)
                && !path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir)),
            "查询路径越出数据目录"
        );
        let mut ancestor = path;
        while !ancestor.exists() {
            ancestor = ancestor.parent().context("查询路径无效")?;
        }
        ensure!(
            ancestor
                .canonicalize()?
                .starts_with(self.root.canonicalize()?),
            "查询路径越出数据目录"
        );
        Ok(())
    }
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.len() <= MAX_TEXT, "SQL 查询超过 8 MiB");
    let mut bytes = Vec::new();
    file.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_TEXT,
        "SQL 查询读取时增长超过 8 MiB"
    );
    Ok(bytes)
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
pub fn stamp(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD
        .encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}
fn file_stamp(path: &Path) -> Result<Option<String>> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(file.metadata()?.len() <= MAX_TEXT, "SQL 查询超过 8 MiB");
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0; 64 * 1024];
    let mut count = 0u64;
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        count += size as u64;
        ensure!(count <= MAX_TEXT, "SQL 查询读取时增长超过 8 MiB");
        digest.update(&buffer[..size]);
    }
    Ok(Some(
        base64::engine::general_purpose::STANDARD.encode(digest.finish()),
    ))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("查询父目录不存在")?)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
