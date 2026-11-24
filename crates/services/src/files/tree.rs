//! Local connection-tree mutations. All filesystem work stays off the UI thread.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    CreateMarkdown { parent: PathBuf, name: String },
    CreateDirectory { parent: PathBuf, name: String },
    Rename { path: PathBuf, name: String },
    Move { path: PathBuf, parent: PathBuf },
    Delete { paths: Vec<PathBuf> },
}
impl Action {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::CreateMarkdown { parent, name } | Self::CreateDirectory { parent, name } => {
                parent.capacity().saturating_add(name.capacity())
            }
            Self::Rename { path, name } => path.capacity().saturating_add(name.capacity()),
            Self::Move { path, parent } => path.capacity().saturating_add(parent.capacity()),
            Self::Delete { paths } => paths.iter().fold(
                paths
                    .capacity()
                    .saturating_mul(std::mem::size_of::<PathBuf>()),
                |sum, path| sum.saturating_add(path.capacity()),
            ),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Changed {
    pub created: Option<PathBuf>,
    pub renamed: Option<(PathBuf, PathBuf)>,
    pub deleted: Vec<PathBuf>,
    pub refresh: Vec<PathBuf>,
    pub errors: Vec<String>,
}
fn name(value: &str) -> Result<&str> {
    let value = value.trim();
    ensure!(
        !value.is_empty()
            && !value.contains(['/', '\\', ':', '\0'])
            && value != "."
            && value != "..",
        "名称不能包含路径分隔符"
    );
    Ok(value)
}
fn inside(root: &Path, path: &Path, parent: bool) -> Result<()> {
    ensure!(
        path.is_absolute()
            && path.starts_with(root)
            && !path.components().any(|c| matches!(c, Component::ParentDir)),
        "路径不属于此连接"
    );
    let resolved = if parent {
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("不能修改连接根目录"))?
    } else {
        path
    };
    ensure!(
        resolved.canonicalize()?.starts_with(root.canonicalize()?),
        "路径越出连接根目录"
    );
    Ok(())
}
pub fn execute(root: &Path, action: Action) -> Result<Changed> {
    ensure!(root.is_absolute() && root.is_dir(), "目录不存在或不可访问");
    let mut changed = Changed {
        created: None,
        renamed: None,
        deleted: Vec::new(),
        refresh: Vec::new(),
        errors: Vec::new(),
    };
    match action {
        Action::CreateMarkdown {
            parent,
            name: input,
        } => {
            inside(root, &parent, false)?;
            let mut filename = name(&input)?.to_owned();
            let extension = Path::new(&filename)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("");
            if !["md", "markdown", "mdown", "mkd"]
                .iter()
                .any(|e| extension.eq_ignore_ascii_case(e))
            {
                filename.push_str(".md");
            }
            let path = parent.join(filename);
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            changed.created = Some(path);
            changed.refresh.push(parent);
        }
        Action::CreateDirectory {
            parent,
            name: input,
        } => {
            inside(root, &parent, false)?;
            std::fs::create_dir(parent.join(name(&input)?))?;
            changed.refresh.push(parent);
        }
        Action::Rename { path, name: input } => {
            ensure!(path != root, "不能重命名连接根目录");
            inside(root, &path, true)?;
            let target = path.with_file_name(name(&input)?);
            ensure!(!target.try_exists()?, "已存在同名项：{}", target.display());
            std::fs::rename(&path, &target)?;
            changed.refresh.push(path.parent().unwrap().to_owned());
            changed.renamed = Some((path, target));
        }
        Action::Move { path, parent } => {
            ensure!(path != root, "不能移动连接根目录");
            inside(root, &path, true)?;
            inside(root, &parent, false)?;
            ensure!(parent.is_dir(), "目标不是目录");
            let source = path.canonicalize()?;
            let destination = parent.canonicalize()?;
            ensure!(!destination.starts_with(&source), "不能移动到自身或子目录");
            let target = parent.join(
                path.file_name()
                    .ok_or_else(|| anyhow::anyhow!("源名称为空"))?,
            );
            ensure!(
                target != path && !target.try_exists()?,
                "目标目录已存在同名项"
            );
            std::fs::rename(&path, &target)?;
            changed.refresh.push(path.parent().unwrap().to_owned());
            changed.refresh.push(parent);
            changed.renamed = Some((path, target));
        }
        Action::Delete { mut paths } => {
            ensure!(paths.len() <= 10_000, "单次删除最多 10,000 项");
            for path in &paths {
                ensure!(path != root, "不能删除连接根目录");
                inside(root, path, true)?;
            }
            paths.sort();
            paths.dedup();
            let mut roots: Vec<PathBuf> = Vec::new();
            for path in paths {
                if !roots.iter().any(|p| path.starts_with(p)) {
                    roots.push(path);
                }
            }
            for path in roots {
                changed.refresh.push(path.parent().unwrap().to_owned());
                let result = std::fs::symlink_metadata(&path).and_then(|metadata| {
                    if metadata.is_dir() && !metadata.file_type().is_symlink() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    }
                });
                match result {
                    Ok(()) => changed.deleted.push(path),
                    Err(error) => changed.errors.push(format!("{}: {error}", path.display())),
                }
            }
        }
    }
    changed.refresh.sort();
    changed.refresh.dedup();
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_refresh_both_parents_and_reject_overwrite_descendants_and_escape() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        let a = root.join("甲");
        let b = root.join("乙");
        std::fs::create_dir_all(a.join("子目录"))?;
        std::fs::create_dir(&b)?;
        let file = a.join("中文.md");
        std::fs::write(&file, "contents")?;
        std::fs::write(b.join("中文.md"), "existing")?;
        assert!(
            execute(
                root,
                Action::Move {
                    path: file.clone(),
                    parent: b.clone()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&file)?, "contents");
        assert!(
            execute(
                root,
                Action::Move {
                    path: a.clone(),
                    parent: a.join("子目录")
                }
            )
            .is_err()
        );
        let changed = execute(
            root,
            Action::Move {
                path: a.clone(),
                parent: b.clone(),
            },
        )?;
        assert_eq!(changed.refresh, [root.to_path_buf(), b.clone()]);
        assert_eq!(std::fs::read_to_string(b.join("甲/中文.md"))?, "contents");
        assert!(
            execute(
                root,
                Action::Move {
                    path: b.clone(),
                    parent: root.join("..")
                }
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn creates_renames_and_deletes_without_overwriting_or_deleting_root() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let created = execute(
            root,
            Action::CreateMarkdown {
                parent: root.to_owned(),
                name: "文档".into(),
            },
        )?
        .created
        .unwrap();
        assert_eq!(created, root.join("文档.md"));
        std::fs::write(&created, "# 内容")?;
        assert!(
            execute(
                root,
                Action::CreateMarkdown {
                    parent: root.to_owned(),
                    name: "文档.md".into()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&created)?, "# 内容");
        execute(
            root,
            Action::CreateDirectory {
                parent: root.to_owned(),
                name: "子目录".into(),
            },
        )?;
        execute(
            root,
            Action::Rename {
                path: created,
                name: "改名.MD".into(),
            },
        )?;
        let child = root.join("子目录");
        std::fs::write(child.join("nested.md"), "child")?;
        let result = execute(
            root,
            Action::Delete {
                paths: vec![child.join("nested.md"), child.clone(), child],
            },
        )?;
        assert_eq!(result.deleted.len(), 1);
        assert!(result.errors.is_empty());
        assert!(root.join("改名.MD").exists());
        assert!(
            execute(
                root,
                Action::Delete {
                    paths: vec![root.to_owned()]
                }
            )
            .is_err()
        );
        assert!(
            execute(
                root,
                Action::CreateMarkdown {
                    parent: root.to_owned(),
                    name: "../outside".into()
                }
            )
            .is_err()
        );
        assert!(
            execute(
                root,
                Action::CreateDirectory {
                    parent: root.join(".."),
                    name: "outside".into()
                }
            )
            .is_err()
        );
        Ok(())
    }
}
