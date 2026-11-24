//! Development use cases shared by the command facade, desktop and CLI.
mod build;
mod process;
pub mod repository;
mod runtime;
#[cfg(test)]
mod tests;
mod version;
mod workspace;
use anyhow::{Result, ensure};
pub use process::Log;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tomato_core::development::{Config, Project};
pub use version::{Action as GitAction, Push};
pub use version::{Change, Versions};
pub use workspace::{Entry, Overview, RunItem};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Load,
    Save {
        config: Box<Config>,
        revision: Option<String>,
    },
    Inspect {
        project: Project,
    },
    InspectDirectory {
        path: PathBuf,
    },
    List {
        root: PathBuf,
        path: PathBuf,
    },
    Read {
        root: PathBuf,
        path: PathBuf,
    },
    ReadVersion {
        root: PathBuf,
        path: String,
    },
    Diff {
        root: PathBuf,
        path: PathBuf,
        revision: String,
    },
    Write {
        root: PathBuf,
        path: PathBuf,
        text: String,
        revision: String,
    },
    Versions {
        root: PathBuf,
        realtime: bool,
        reset: bool,
    },
    Git {
        root: PathBuf,
        action: version::Action,
    },
    Launch {
        config: Box<Config>,
        project: Project,
        item: RunItem,
        restart: bool,
    },
    Stop {
        root: PathBuf,
    },
    Poll {
        root: PathBuf,
        after: u64,
        #[serde(default)]
        dependencies: bool,
    },
    ClearLog {
        root: PathBuf,
    },
    Dependencies {
        config: Box<Config>,
        project: Project,
    },
    DetectVersion {
        kind: String,
        path: PathBuf,
    },
    ValidateEntry {
        role: tomato_core::development::runtime::EntryRole,
        kind: String,
        name: String,
        path: PathBuf,
        settings: PathBuf,
    },
    Shutdown,
}
#[derive(Debug, Serialize)]
pub enum Output {
    Config {
        config: Config,
        revision: Option<String>,
    },
    Overview(Overview),
    Directory {
        valid: bool,
        project_type: String,
    },
    Entries {
        path: PathBuf,
        entries: Vec<Entry>,
    },
    File {
        path: PathBuf,
        text: String,
        revision: String,
        read_only: bool,
    },
    Written {
        path: PathBuf,
        revision: String,
    },
    Diff {
        path: PathBuf,
        revision: String,
        changes: Vec<tomato_core::development::LineChange>,
    },
    Versions(Versions),
    Log(Log),
    EntryValidated {
        message: Option<String>,
    },
    Text(String),
}
#[derive(Default)]
pub struct Manager {
    processes: process::Processes,
    dependencies: process::Processes,
    snapshots: HashMap<PathBuf, version::Snapshot>,
}
impl Manager {
    pub async fn execute(&mut self, path: PathBuf, action: Action) -> Result<Output> {
        match action {
            Action::Load => {
                let (config, revision) = repository::load(&path)?;
                Ok(Output::Config { config, revision })
            }
            Action::Save { config, revision } => {
                let revision = repository::save(&path, &config, revision.as_deref())?;
                Ok(Output::Config {
                    config: *config,
                    revision: Some(revision),
                })
            }
            Action::Inspect { project } => Ok(Output::Overview(workspace::inspect(&project)?)),
            Action::InspectDirectory { path } => Ok(Output::Directory {
                valid: path.is_dir(),
                project_type: workspace::detect(&path),
            }),
            Action::List { root, path } => Ok(Output::Entries {
                entries: workspace::list(&root, &path)?,
                path,
            }),
            Action::Read { root, path } => {
                let (text, revision) = workspace::read(&root, &path)?;
                Ok(Output::File {
                    path,
                    text,
                    revision,
                    read_only: false,
                })
            }
            Action::ReadVersion { root, path } => {
                let text =
                    version::execute(&root, version::Action::Base { path: path.clone() }).await?;
                let revision = repository::digest(text.as_bytes());
                Ok(Output::File {
                    path: root.join(path),
                    text,
                    revision,
                    read_only: true,
                })
            }
            Action::Diff {
                root,
                path,
                revision,
            } => {
                let changes = version::highlights(&root, &path, &revision)
                    .await
                    .unwrap_or_default();
                Ok(Output::Diff {
                    path,
                    revision,
                    changes,
                })
            }
            Action::Write {
                root,
                path,
                text,
                revision,
            } => {
                let revision = workspace::write(&root, &path, &text, &revision)?;
                Ok(Output::Written { path, revision })
            }
            Action::Versions {
                root,
                realtime,
                reset,
            } => {
                let result = if realtime {
                    version::realtime(&root, &mut self.snapshots, reset)?
                } else {
                    version::git_changes(&root).await?
                };
                Ok(Output::Versions(result))
            }
            Action::Git { root, action } => {
                Ok(Output::Text(version::execute(&root, action).await?))
            }
            Action::Launch {
                config,
                project,
                item,
                restart,
            } => {
                self.processes
                    .launch(*config, project, item, restart)
                    .await?;
                Ok(Output::Text("启动任务已提交".into()))
            }
            Action::Stop { root } => {
                self.processes.stop(&root).await;
                self.dependencies.stop(&root).await;
                Ok(Output::Log(self.processes.poll(&root, 0)))
            }
            Action::Poll {
                root,
                after,
                dependencies,
            } => Ok(Output::Log(if dependencies {
                self.dependencies.poll(&root, after)
            } else {
                self.processes.poll(&root, after)
            })),
            Action::ClearLog { root } => {
                self.processes.clear(&root);
                Ok(Output::Log(self.processes.poll(&root, 0)))
            }
            Action::Dependencies { config, project } => {
                self.dependencies.dependencies(*config, project).await?;
                Ok(Output::Text("正在加载构建依赖...".into()))
            }
            Action::DetectVersion { kind, path } => {
                Ok(Output::Text(runtime::detect(&kind, &path).await?))
            }
            Action::ValidateEntry {
                role,
                kind,
                name,
                path,
                settings,
            } => Ok(Output::EntryValidated {
                message: runtime::validate(role, &kind, &name, &path, &settings),
            }),
            Action::Shutdown => {
                self.processes.shutdown().await;
                self.dependencies.shutdown().await;
                self.snapshots.clear();
                Ok(Output::Text("开发任务已停止".into()))
            }
        }
    }
}
