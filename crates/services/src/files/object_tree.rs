//! Object-directory use cases share the native storage adapter, not local disk paths.
use super::{
    Entry,
    operations::{Operation, Output as FileOutput},
    storage::Storage,
    tree::{Action, Changed},
};
use crate::{commands::Output, markdown::resource::Source};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tomato_core::Connection;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    List(PathBuf),
    Read(PathBuf),
    Write { path: PathBuf, text: String },
    Image { document: PathBuf, source: String },
    Mutate(Action),
}
impl Request {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::List(path) | Self::Read(path) => path.capacity(),
            Self::Write { path, text } => path.capacity().saturating_add(text.capacity()),
            Self::Image { document, source } => {
                document.capacity().saturating_add(source.capacity())
            }
            Self::Mutate(action) => action.queued_bytes(),
        }
    }
}
pub fn is_object(c: &Connection) -> bool {
    c.kind() == "LOCAL_DIRECTORY" && c.text("directoryType") == "S3"
}
fn key(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .context("对象路径不是 UTF-8")?
        .replace('\\', "/");
    ensure!(
        value.starts_with('/') && !value.contains('\0'),
        "对象路径必须从连接根开始"
    );
    ensure!(
        !value.split('/').any(|part| matches!(part, "." | "..")),
        "对象路径不能越出连接根"
    );
    Ok(value.trim_end_matches('/').to_owned())
}
fn display(value: &str) -> PathBuf {
    PathBuf::from(if value.is_empty() { "/" } else { value })
}
fn name(value: &str) -> Result<&str> {
    let value = value.trim();
    ensure!(
        !value.is_empty() && !value.contains(['/', '\\', '\0']) && !matches!(value, "." | ".."),
        "名称不能包含路径分隔符"
    );
    Ok(value)
}
async fn available(storage: &Storage, path: &str) -> Result<()> {
    ensure!(
        !storage.exists(path.trim_start_matches('/')).await?,
        "目标已存在"
    );
    Ok(())
}
async fn directory(storage: &Storage, path: &str) -> Result<bool> {
    if path.is_empty() {
        return Ok(true);
    }
    match storage.metadata(path.trim_start_matches('/')).await {
        Ok(meta) => Ok(meta.is_dir()),
        Err(error) if error.kind() == opendal::ErrorKind::NotFound => Ok(storage
            .metadata(&format!("{}/", path.trim_matches('/')))
            .await?
            .is_dir()),
        Err(error) => Err(error.into()),
    }
}
pub async fn execute(connection: &Connection, request: Request) -> Result<Output> {
    ensure!(is_object(connection), "不是 S3 目录连接");
    ensure!(
        !connection.text("bucket").is_empty(),
        "S3 目录连接需要配置 Bucket"
    );
    match request {
        Request::List(path) => {
            let path = key(&path)?;
            let storage = Storage::connect(connection).await?;
            let FileOutput::Directory { entries, .. } = storage.list(&path).await? else {
                anyhow::bail!("目录结果不匹配")
            };
            ensure!(entries.len() <= 5000, "连接树单个目录最多加载 5,000 项");
            let entries: Vec<_> = entries
                .into_iter()
                .map(|e| Entry {
                    name: e.name,
                    path: display(e.path.trim_end_matches('/')),
                    directory: e.directory,
                    bytes: e.size,
                })
                .collect();
            ensure!(
                entries.iter().map(Entry::retained_bytes).sum::<usize>() <= 8 * 1024 * 1024,
                "目录树列表超过 8 MiB"
            );
            Ok(Output::Files(entries))
        }
        Request::Read(path) => {
            let storage = Storage::connect(connection).await?;
            let FileOutput::Text { text, .. } =
                storage.execute(Operation::ReadText(key(&path)?)).await?
            else {
                anyhow::bail!("文本结果不匹配")
            };
            Ok(Output::Text(text))
        }
        Request::Write { path, text } => {
            let storage = Storage::connect(connection).await?;
            storage
                .execute(Operation::WriteText {
                    path: key(&path)?,
                    text,
                })
                .await?;
            Ok(Output::Text("文件已保存".into()))
        }
        Request::Image { document, source } => {
            let direct = Source::parse(&source)?;
            let Source::Relative(_) = direct else {
                return Ok(Output::MarkdownImage(direct.picture().await?));
            };
            let storage = Storage::connect(connection).await?;
            let FileOutput::Image { asset, .. } = storage
                .execute(Operation::ReadDocumentImage {
                    document: key(&document)?,
                    source,
                })
                .await?
            else {
                anyhow::bail!("图片结果不匹配");
            };
            Ok(Output::MarkdownImage(asset.picture))
        }
        Request::Mutate(action) => {
            let storage = Storage::connect(connection).await?;
            Ok(Output::LocalTree(mutate(&storage, action).await?))
        }
    }
}
async fn mutate(storage: &Storage, action: Action) -> Result<Changed> {
    let mut changed = Changed {
        created: None,
        renamed: None,
        deleted: Vec::new(),
        refresh: Vec::new(),
        errors: Vec::new(),
    };
    let markdown = matches!(&action, Action::CreateMarkdown { .. });
    match action {
        Action::CreateDirectory {
            parent,
            name: input,
        }
        | Action::CreateMarkdown {
            parent,
            name: input,
        } => {
            ensure!(directory(storage, &key(&parent)?).await?, "父节点不是目录");
            let mut filename = name(&input)?.to_owned();
            if markdown
                && !["md", "markdown", "mdown", "mkd"].iter().any(|extension| {
                    Path::new(&filename)
                        .extension()
                        .and_then(|v| v.to_str())
                        .is_some_and(|v| v.eq_ignore_ascii_case(extension))
                })
            {
                filename.push_str(".md");
            }
            let path = parent.join(filename);
            let target = key(&path)?;
            available(storage, &target).await?;
            storage
                .execute(if markdown {
                    Operation::Create(target)
                } else {
                    Operation::Mkdir(target)
                })
                .await?;
            if markdown {
                changed.created = Some(path);
            }
            changed.refresh.push(parent);
        }
        Action::Rename { path, name: input } => {
            let from = key(&path)?;
            ensure!(!from.is_empty(), "不能重命名连接根目录");
            let target = path.with_file_name(name(&input)?);
            move_object(storage, &path, &target).await?;
            changed.refresh.push(path.parent().unwrap().to_owned());
            changed.renamed = Some((path, target));
        }
        Action::Move { path, parent } => {
            ensure!(directory(storage, &key(&parent)?).await?, "目标不是目录");
            let target = parent.join(path.file_name().context("源名称为空")?);
            move_object(storage, &path, &target).await?;
            changed
                .refresh
                .extend([path.parent().context("不能移动连接根")?.to_owned(), parent]);
            changed.renamed = Some((path, target));
        }
        Action::Delete { mut paths } => {
            ensure!(paths.len() <= 10_000, "单次删除最多 10,000 项");
            for path in &paths {
                ensure!(!key(path)?.is_empty(), "不能删除连接根目录");
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
                let key = key(&path)?;
                let result = async {
                    let dir = directory(storage, &key).await?;
                    storage
                        .execute(Operation::Remove {
                            path: key,
                            directory: dir,
                        })
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                match result {
                    Ok(()) => changed.deleted.push(path),
                    Err(e) => changed.errors.push(format!("{}: {e:#}", path.display())),
                }
            }
        }
    }
    changed.refresh.sort();
    changed.refresh.dedup();
    Ok(changed)
}
async fn move_object(storage: &Storage, source: &Path, target: &Path) -> Result<()> {
    let from = key(source)?;
    let to = key(target)?;
    ensure!(
        !from.is_empty() && !to.is_empty() && !target.starts_with(source),
        "不能移动连接根、移动到自身或子目录"
    );
    available(storage, &to).await?;
    let dir = directory(storage, &from).await?;
    storage
        .execute(Operation::Rename {
            from: if dir { format!("{from}/") } else { from },
            to,
        })
        .await?;
    Ok(())
}
