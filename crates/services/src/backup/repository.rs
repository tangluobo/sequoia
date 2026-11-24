//! Legacy backup filesystem repository; archive bytes are read through a seek index.
use super::Archive;
use anyhow::{Context, Result, ensure};
use std::{
    path::{Component, Path, PathBuf},
    sync::Mutex,
};
use tomato_core::backup::{FileAction, FileOutput, FileRequest, Item, Scope};
static WRITES: Mutex<()> = Mutex::new(());
pub struct Repository {
    root: PathBuf,
}
impl Repository {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn inside(&self, path: &Path) -> Result<()> {
        ensure!(
            self.root.is_absolute()
                && path.starts_with(&self.root)
                && !path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir)),
            "备份路径越出数据目录"
        );
        let mut ancestor = path;
        while !ancestor.exists() {
            ancestor = ancestor.parent().context("备份路径无效")?;
        }
        ensure!(
            ancestor
                .canonicalize()?
                .starts_with(self.root.canonicalize()?),
            "备份路径越出数据目录"
        );
        Ok(())
    }
    fn prepare(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        self.inside(path)?;
        std::fs::create_dir_all(path)?;
        self.inside(path)
    }
    fn item(&self, scope: &Scope, item: &Item) -> Result<PathBuf> {
        Ok(self.root.join(if item.directory {
            scope.child(&item.name)?.directory()?
        } else {
            scope.file(&item.name)?
        }))
    }
    pub(crate) fn create_path(&self, scope: &Scope, filename: &str) -> Result<PathBuf> {
        ensure!(self.root.is_absolute(), "备份数据目录需要绝对路径");
        let directory = self.root.join(scope.directory()?);
        self.prepare(&directory)?;
        let file = self.root.join(scope.create_file(Some(filename))?);
        self.inside(&file)?;
        Ok(file)
    }
    pub(crate) fn read_path(&self, scope: &Scope, name: &str) -> Result<PathBuf> {
        let file = self.root.join(scope.file(name)?);
        ensure!(file.is_file(), "备份文件不存在");
        self.inside(&file)?;
        Ok(file)
    }
    pub fn execute(&self, request: FileRequest) -> Result<FileOutput> {
        ensure!(self.root.is_absolute(), "备份数据目录需要绝对路径");
        ensure!(
            request.retained_bytes() <= 8 * 1024 * 1024,
            "备份文件请求超过 8 MiB"
        );
        let _guard = if matches!(&request.action, FileAction::List | FileAction::Info { .. }) {
            None
        } else {
            Some(
                WRITES
                    .lock()
                    .map_err(|_| anyhow::anyhow!("备份文件写入锁失效"))?,
            )
        };
        let FileRequest { scope, action } = request;
        let directory = self.root.join(scope.directory()?);
        match action {
            FileAction::List => {
                if !directory.is_dir() {
                    return Ok(FileOutput::Listed {
                        scope,
                        items: Vec::new(),
                    });
                }
                self.inside(&directory)?;
                let mut directories = Vec::new();
                let mut backups = Vec::new();
                let mut bytes = 0usize;
                for entry in std::fs::read_dir(&directory)? {
                    let entry = entry?;
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("备份名称不是 Unicode"))?;
                    if entry.path().is_dir() {
                        bytes = bytes.saturating_add(name.capacity() + std::mem::size_of::<Item>());
                        directories.push(Item {
                            name: name.clone(),
                            directory: true,
                        });
                    }
                    if let Some(base) = name.strip_suffix(".nb3") {
                        bytes = bytes.saturating_add(base.len() + std::mem::size_of::<Item>());
                        backups.push(Item {
                            name: base.into(),
                            directory: false,
                        });
                    }
                    ensure!(
                        bytes <= 8 * 1024 * 1024 && directories.len() + backups.len() <= 10_000,
                        "备份目录超过 10,000 项或 8 MiB"
                    );
                }
                directories.extend(backups);
                Ok(FileOutput::Listed {
                    scope,
                    items: directories,
                })
            }
            FileAction::Info { name } => {
                let file = self.root.join(scope.file(&name)?);
                self.inside(&file)?;
                let bytes = std::fs::metadata(&file)?.len();
                let mut archive = Archive::open(&file)?;
                let metadata = archive.json("meta.json", false)?;
                Ok(FileOutput::Info {
                    scope,
                    name,
                    metadata: Box::new(metadata),
                    bytes,
                })
            }
            FileAction::Directory => {
                self.prepare(&directory)?;
                Ok(FileOutput::Directory { path: directory })
            }
            FileAction::CreateDirectory { name } => {
                let child = self.root.join(scope.child(&name)?.directory()?);
                self.prepare(&child)?;
                Ok(FileOutput::Changed {
                    scope,
                    renamed: None,
                    deleted: Vec::new(),
                    errors: Vec::new(),
                })
            }
            FileAction::Rename { item, name } => {
                let from = self.item(&scope, &item)?;
                self.inside(&from)?;
                let to = self.root.join(if item.directory {
                    scope.child(&name)?.directory()?
                } else {
                    scope.renamed_file(&name)?
                });
                self.inside(&to)?;
                ensure!(!to.exists(), "同名备份或目录已存在");
                std::fs::rename(&from, &to)?;
                let new = Item {
                    name: if item.directory {
                        scope
                            .child(&name)?
                            .path
                            .rsplit('/')
                            .next()
                            .unwrap_or_default()
                            .into()
                    } else {
                        to.file_stem()
                            .context("备份名称无效")?
                            .to_string_lossy()
                            .into_owned()
                    },
                    directory: item.directory,
                };
                Ok(FileOutput::Changed {
                    scope,
                    renamed: Some((item, new)),
                    deleted: Vec::new(),
                    errors: Vec::new(),
                })
            }
            FileAction::Delete { items } => {
                let mut deleted = Vec::new();
                let mut errors = Vec::new();
                for item in items {
                    let result = (|| {
                        let path = self.item(&scope, &item)?;
                        self.inside(&path)?;
                        if !path.exists() {
                            return Ok(());
                        }
                        if item.directory {
                            std::fs::remove_dir_all(&path)?;
                        } else {
                            std::fs::remove_file(&path)?;
                        }
                        Ok::<_, anyhow::Error>(())
                    })();
                    match result {
                        Ok(()) => deleted.push(item),
                        Err(error) => errors.push(format!("{}: {error}", item.name)),
                    }
                }
                Ok(FileOutput::Changed {
                    scope,
                    renamed: None,
                    deleted,
                    errors,
                })
            }
        }
    }
}
