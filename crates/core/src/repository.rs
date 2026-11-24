//! Repository port: use cases do not depend on a concrete storage format.
use crate::Store;
use anyhow::{Context, Result, bail};
use std::{
    cell::RefCell,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
mod guard;
use guard::fingerprint;
pub use guard::{SaveGuard, SaveReceipt};

pub trait WorkspaceRepository {
    fn load(&self) -> Result<Store>;
    fn save(&self, store: &Store) -> Result<()>;
    fn save_guard(&self) -> Result<Option<SaveGuard>> {
        Ok(None)
    }
    fn accept_saved(&self, _receipt: SaveReceipt) -> Result<()> {
        bail!("此配置仓库不支持后台保存")
    }
    fn location(&self) -> Option<&Path> {
        None
    }
    fn export(&self, store: &Store, path: &Path) -> Result<()> {
        write_export(self.location(), path, store.export()?.as_bytes())
    }
    fn export_prepared(
        &self,
        prepared: &crate::config::export::Prepared,
        path: &Path,
    ) -> Result<()> {
        write_export(self.location(), path, prepared.encode()?.as_bytes())
    }
}

pub fn write_export(source: Option<&Path>, path: &Path, bytes: &[u8]) -> Result<()> {
    write_export_checked(source, path, bytes, || Ok(()))
}
pub fn write_export_checked(
    source: Option<&Path>,
    path: &Path,
    bytes: &[u8],
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    if let Some(source) = source {
        let source = source
            .canonicalize()
            .or_else(|_| std::path::absolute(source))?;
        let target = path.canonicalize().or_else(|_| std::path::absolute(path))?;
        if source == target {
            bail!("导出目标不能覆盖正在使用的连接配置，请选择其他文件");
        }
    }
    publish(path, bytes, check, || Ok(()))
}

fn publish(
    path: &Path,
    bytes: &[u8],
    check: impl Fn() -> Result<()>,
    before_publish: impl Fn() -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    for chunk in bytes.chunks(64 * 1024) {
        check()?;
        temporary.write_all(chunk)?;
    }
    temporary.as_file().sync_all()?;
    check()?;
    before_publish()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub struct ConnectionFileRepository {
    path: PathBuf,
    revision: RefCell<Revision>,
}
#[derive(Clone, Copy)]
enum Revision {
    Unloaded,
    Loaded(Option<[u8; 32]>),
}
/// Retained for callers that previously used the Rust-only JSON repository name.
pub type JsonWorkspaceRepository = ConnectionFileRepository;

impl ConnectionFileRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            revision: RefCell::new(Revision::Unloaded),
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn backup_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(".before-rust.bak");
        PathBuf::from(name)
    }
    fn read(&self) -> Result<Option<Vec<u8>>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("无法读取连接配置"),
        }
    }
    fn check_revision(&self, current: &Option<Vec<u8>>) -> Result<()> {
        match &*self.revision.borrow() {
            Revision::Loaded(previous) if *previous == fingerprint(current.as_deref()) => Ok(()),
            Revision::Unloaded => bail!("请先加载连接配置再保存"),
            _ => bail!("连接配置已被其他程序修改，请重新打开应用后再编辑；本次未覆盖外部修改"),
        }
    }
    fn backup(&self, original: &[u8]) -> Result<()> {
        let backup_path = self.backup_path();
        if backup_path.exists() {
            if !backup_path.is_file() {
                bail!("备份路径不是文件，保存已取消");
            }
            return Ok(());
        }
        // Existing encrypted files are backed up byte-for-byte. Older plaintext
        // variants are backed up as Java-compatible encrypted data, never as new plaintext.
        let encrypted;
        let bytes = if original.starts_with(crate::legacy::MARKER.as_bytes()) {
            original
        } else {
            encrypted = crate::legacy::encode(&Store::decode(original)?.connections)?;
            encrypted.as_bytes()
        };
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        match temporary.persist_noclobber(backup_path) {
            Ok(_) => Ok(()),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error.error).context("无法备份原连接配置，保存已取消"),
        }
    }
}
impl WorkspaceRepository for ConnectionFileRepository {
    fn location(&self) -> Option<&Path> {
        Some(&self.path)
    }
    fn load(&self) -> Result<Store> {
        let current = self.read()?;
        let store = match &current {
            Some(bytes) => Store::decode(bytes)?,
            None => Store {
                version: 1,
                ..Store::default()
            },
        };
        *self.revision.borrow_mut() = Revision::Loaded(fingerprint(current.as_deref()));
        Ok(store)
    }
    fn save(&self, store: &Store) -> Result<()> {
        self.save_checked(store, || Ok(()))
    }
    fn save_guard(&self) -> Result<Option<SaveGuard>> {
        Ok(Some(self.guard()?))
    }
    fn accept_saved(&self, receipt: SaveReceipt) -> Result<()> {
        anyhow::ensure!(
            self.guard()? == receipt.base,
            "工作区在后台保存期间已变化，请重新加载连接配置"
        );
        *self.revision.borrow_mut() = Revision::Loaded(receipt.revision);
        Ok(())
    }
}
impl ConnectionFileRepository {
    pub fn save_checked(&self, store: &Store, check: impl Fn() -> Result<()>) -> Result<()> {
        check()?;
        crate::config::validate(&store.connections)?;
        let current = self.read()?;
        self.check_revision(&current)?;
        let encoded = crate::legacy::encode(&store.connections)?.into_bytes();
        check()?;
        if current.as_deref() == Some(encoded.as_slice()) {
            return Ok(());
        }
        if let Some(original) = &current {
            self.backup(original)?;
        }
        // Recheck after backup creation; Java does not participate in our locking.
        self.check_revision(&self.read()?)?;
        publish(&self.path, &encoded, check, || {
            self.check_revision(&self.read()?)
        })?;
        *self.revision.borrow_mut() = Revision::Loaded(fingerprint(Some(&encoded)));
        Ok(())
    }
}
