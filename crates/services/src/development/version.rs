use super::process::{self, Plan};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Component, Path, PathBuf},
    time::Duration,
};
pub type Snapshot = BTreeMap<String, (u64, u128)>;
#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub path: String,
    pub status: String,
}
#[derive(Debug, Serialize)]
pub struct Versions {
    pub root: PathBuf,
    pub git: bool,
    pub changes: Vec<Change>,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Commit {
        files: Vec<String>,
        message: String,
        push: Option<Push>,
    },
    Rollback {
        files: Vec<String>,
    },
    Ignore {
        path: String,
        directory: bool,
    },
    Base {
        path: String,
    },
    Remotes,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Push {
    pub remote: String,
    pub branch: String,
}
async fn git(root: &Path, args: Vec<String>) -> Result<String> {
    let mut plan = Plan::new("git", root);
    plan.args = args;
    process::capture(plan, Duration::from_secs(120)).await
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
pub fn checked(root: &Path, path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    ensure!(
        !path.is_empty()
            && !path.contains('\0')
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "无效的项目相对路径"
    );
    let mut target = root.join(relative);
    while !target.exists() {
        ensure!(target.pop(), "无效项目路径");
    }
    super::workspace::confined(root, &target)?;
    Ok(root.join(relative))
}
pub async fn git_changes(root: &Path) -> Result<Versions> {
    if git(root, strings(&["rev-parse", "--is-inside-work-tree"]))
        .await
        .is_err()
    {
        return Ok(Versions {
            root: root.into(),
            git: false,
            changes: vec![],
            message: "当前目录不是 git 仓库（或 git 命令不可用）".into(),
        });
    }
    let prefix = git(root, strings(&["rev-parse", "--show-prefix"])).await?;
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    let text = git(
        root,
        strings(&[
            "-c",
            "core.quotepath=false",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--",
            ".",
        ]),
    )
    .await?;
    let mut parts = text.split('\0');
    let mut changes = Vec::new();
    while let Some(part) = parts.next() {
        if part.len() < 4 || !part.is_char_boundary(3) {
            continue;
        }
        let code = &part[..2];
        let path = part[3..].strip_prefix(prefix).unwrap_or(&part[3..]);
        if code.contains(['R', 'C']) {
            parts.next();
        }
        let status = if code.contains('U') {
            "冲突"
        } else if code.contains('A') {
            "已添加"
        } else if code.contains('M') {
            "变更"
        } else if code.contains('D') {
            "已删除"
        } else if code.contains('R') {
            "重命名"
        } else if code.contains('C') {
            "拷贝"
        } else if code.contains('?') {
            "未添加"
        } else {
            "变更"
        };
        changes.push(Change {
            path: path.into(),
            status: status.into(),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    let message = if changes.is_empty() {
        "当前 git 仓库无待提交文件"
    } else {
        "已读取 git 变更列表"
    }
    .into();
    Ok(Versions {
        root: root.into(),
        git: true,
        changes,
        message,
    })
}
fn scan(root: &Path) -> Result<Snapshot> {
    let mut result = BTreeMap::new();
    super::workspace::walk(root, |path, meta| {
        let path = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let time = meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        result.insert(path, (meta.len(), time));
        Ok(())
    })?;
    Ok(result)
}
pub fn realtime(
    root: &Path,
    snapshots: &mut HashMap<PathBuf, Snapshot>,
    reset: bool,
) -> Result<Versions> {
    let current = scan(root)?;
    if reset || !snapshots.contains_key(root) {
        ensure!(
            snapshots.len() < 32 || snapshots.contains_key(root),
            "实时快照超过 32 个项目"
        );
        snapshots.insert(root.into(), current.clone());
    }
    let base = &snapshots[root];
    let mut changes = Vec::new();
    for (path, stamp) in &current {
        if base.get(path) != Some(stamp) {
            changes.push(Change {
                path: path.clone(),
                status: if base.contains_key(path) {
                    "修改"
                } else {
                    "新增"
                }
                .into(),
            });
        }
    }
    for path in base.keys() {
        if !current.contains_key(path) {
            changes.push(Change {
                path: path.clone(),
                status: "删除".into(),
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    let message = if changes.is_empty() {
        "当前目录无文件变更"
    } else {
        "实时检测到变更，按路径显示"
    }
    .into();
    Ok(Versions {
        root: root.into(),
        git: false,
        changes,
        message,
    })
}
pub async fn execute(root: &Path, action: Action) -> Result<String> {
    match action {
        Action::Commit {
            files,
            message,
            push,
        } => {
            ensure!(
                !files.is_empty() && !message.trim().is_empty(),
                "请选择文件并输入提交说明"
            );
            for path in &files {
                checked(root, path)?;
            }
            let mut args = strings(&["add", "--"]);
            args.extend(files.iter().cloned());
            git(root, args).await?;
            let mut args = vec![
                "commit".into(),
                "--only".into(),
                "-m".into(),
                message,
                "--".into(),
            ];
            args.extend(files);
            let mut result = git(root, args).await?;
            if let Some(push) = push {
                ensure!(
                    !push.remote.starts_with('-')
                        && !push.branch.starts_with('-')
                        && !push.remote.is_empty()
                        && !push.branch.is_empty(),
                    "远程仓库或分支无效"
                );
                result.push_str(
                    &git(
                        root,
                        vec![
                            "push".into(),
                            "-u".into(),
                            push.remote,
                            format!("HEAD:refs/heads/{}", push.branch),
                        ],
                    )
                    .await?,
                );
            }
            Ok(result)
        }
        Action::Rollback { files } => {
            ensure!(!files.is_empty(), "没有选择变更文件");
            let current = git_changes(root).await?;
            for path in files {
                checked(root, &path)?;
                let change = current
                    .changes
                    .iter()
                    .find(|c| c.path == path)
                    .ok_or_else(|| anyhow::anyhow!("文件变更状态已改变，请刷新"))?;
                if change.status == "未添加" || change.status == "已添加" {
                    if change.status == "已添加" {
                        git(
                            root,
                            vec![
                                "restore".into(),
                                "--staged".into(),
                                "--".into(),
                                path.clone(),
                            ],
                        )
                        .await?;
                    }
                    git(root, vec!["clean".into(), "-fd".into(), "--".into(), path]).await?;
                } else {
                    git(
                        root,
                        vec![
                            "restore".into(),
                            "--staged".into(),
                            "--worktree".into(),
                            "--".into(),
                            path,
                        ],
                    )
                    .await?;
                }
            }
            Ok("已回滚选中的变更".into())
        }
        Action::Ignore { path, directory } => {
            let target = checked(root, &path)?;
            let repo = PathBuf::from(
                git(root, strings(&["rev-parse", "--show-toplevel"]))
                    .await?
                    .trim(),
            );
            let actual = std::fs::canonicalize(root)?.join(target.strip_prefix(root)?);
            let relative = actual
                .strip_prefix(std::fs::canonicalize(&repo)?)?
                .to_string_lossy()
                .replace('\\', "/");
            let entry = format!("/{}{}", relative, if directory { "/" } else { "" });
            let ignore = repo.join(".gitignore");
            let mut text = if ignore.exists() {
                String::from_utf8(super::workspace::read_bytes(&ignore, 1024 * 1024)?)?
            } else {
                String::new()
            };
            if !text.lines().any(|line| line.trim() == entry) {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n')
                }
                text.push_str(&entry);
                text.push('\n');
                tomato_core::config::atomic_write(&ignore, text.as_bytes())?;
            }
            Ok("已添加到 .gitignore".into())
        }
        Action::Base { path } => {
            checked(root, &path)?;
            let prefix = git(root, strings(&["rev-parse", "--show-prefix"])).await?;
            git(
                root,
                vec![
                    "show".into(),
                    format!("HEAD:{}{path}", prefix.trim_end_matches(['\r', '\n'])),
                ],
            )
            .await
        }
        Action::Remotes => {
            let mut text = git(root, strings(&["remote"])).await?;
            text.push_str("\n分支：\n");
            text.push_str(&git(root, strings(&["branch", "--format=%(refname:short)"])).await?);
            Ok(text)
        }
    }
}
pub async fn highlights(
    root: &Path,
    path: &Path,
    revision: &str,
) -> Result<Vec<tomato_core::development::LineChange>> {
    let (text, current) = super::workspace::read(root, path)?;
    if current != revision {
        return Ok(vec![]);
    }
    let canonical_root = super::workspace::confined(root, root)?;
    let canonical_path = super::workspace::confined(root, path)?;
    let relative = canonical_path
        .strip_prefix(&canonical_root)?
        .to_string_lossy()
        .replace('\\', "/");
    checked(root, &relative)?;
    let status = git(
        root,
        vec![
            "status".into(),
            "--porcelain=v1".into(),
            "--untracked-files=all".into(),
            "--".into(),
            relative.clone(),
        ],
    )
    .await?;
    if status.trim().is_empty() {
        return Ok(vec![]);
    }
    let new = status.get(..2).is_some_and(|s| s.contains(['?', 'A']));
    let diff = git(
        root,
        vec![
            "diff".into(),
            "--no-ext-diff".into(),
            "--no-color".into(),
            "--unified=0".into(),
            "HEAD".into(),
            "--".into(),
            relative,
        ],
    )
    .await;
    if let Ok(diff) = &diff
        && !diff.trim().is_empty()
    {
        return Ok(tomato_core::development::diff_highlights(diff));
    }
    if new {
        Ok(vec![tomato_core::development::LineChange {
            start: 1,
            end: text.split('\n').count().max(1),
            kind: tomato_core::development::ChangeKind::Added,
        }])
    } else {
        Ok(vec![])
    }
}
