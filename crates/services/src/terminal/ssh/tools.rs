//! Multiplexed SSH operations. Views submit typed requests; protocol I/O stays here.
mod editor;
pub mod management;
mod sftp;
use super::Client;
use anyhow::{Context, Result, ensure};
pub use management::{ManagementAction, Section, Table};
use russh::{ChannelMsg, client};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{OnceCell, mpsc};

pub use crate::files::operations::{Entry, Operation, Output, Reply, Request};
pub(super) struct Contexts {
    pub session: Arc<client::Handle<Client>>,
    pub sftp: OnceCell<Arc<russh_sftp::client::SftpSession>>,
    pub editors: editor::Editors,
}
impl Contexts {
    pub async fn sync_edits(&self) -> Result<Option<String>> {
        let client = self.sftp().await?;
        self.editors.synchronize(&client).await
    }
    pub(super) async fn sftp(&self) -> Result<Arc<russh_sftp::client::SftpSession>> {
        self.sftp
            .get_or_try_init(|| async {
                let channel = self.session.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                let client = tokio::time::timeout(
                    Duration::from_secs(20),
                    russh_sftp::client::SftpSession::new_with_config(
                        channel.into_stream(),
                        russh_sftp::client::Config {
                            max_concurrent_reads: 4,
                            max_concurrent_writes: 4,
                            ..Default::default()
                        },
                    ),
                )
                .await
                .context("SFTP 子系统启动超时")??;
                client.set_timeout(30);
                Ok(Arc::new(client))
            })
            .await
            .cloned()
    }
    pub async fn execute(&self, operation: Operation) -> Result<Output> {
        if let Operation::Batch(operations) = operation {
            ensure!(operations.len() <= 1000, "批量操作超过 1000 项");
            let count = operations.len();
            for (index, operation) in operations.into_iter().enumerate() {
                ensure!(
                    !matches!(operation, Operation::Batch(_)),
                    "不能嵌套批量操作"
                );
                self.execute_single(operation)
                    .await
                    .with_context(|| format!("已完成 {index}/{count} 项，后续操作已停止"))?;
            }
            return Ok(Output::Done(format!("已完成 {count} 项操作")));
        }
        self.execute_single(operation).await
    }
    async fn execute_single(&self, operation: Operation) -> Result<Output> {
        match operation {
            Operation::UploadFiles {
                directory,
                paths,
                deferred,
                job,
            } => {
                let client = self.sftp().await?;
                let report = crate::files::upload::run(
                    &sftp::UploadTarget(&client),
                    &directory,
                    paths,
                    deferred,
                    job,
                )
                .await;
                Ok(Output::Uploaded { directory, report })
            }
            Operation::Query(section) => {
                let command = management::query(section);
                let text = self.exec(&command).await?;
                Ok(Output::Table(management::parse(section, &text)))
            }
            Operation::Manage(action) => {
                let command = management::command(&action)?;
                let text = self.exec(&command).await?;
                if let ManagementAction::DockerInspect(id) = action {
                    let value = management::docker::inspection(&text)?;
                    Ok(Output::Inspection { id, value })
                } else {
                    Ok(Output::Done(text))
                }
            }
            Operation::ClipboardFiles => Ok(Output::ClipboardFiles(
                tokio::task::spawn_blocking(crate::files::clipboard::paths).await??,
            )),
            Operation::PrepareEdit { remote, launch } => {
                let client = self.sftp().await?;
                self.editors.open(&client, remote, launch).await
            }
            Operation::ExportFile(request) => {
                let client = self.sftp().await?;
                request
                    .run(async {
                        request
                            .prepare(|path| {
                                let client = &client;
                                async move {
                                    match sftp::execute(client, Operation::List(path)).await? {
                                        Output::Directory { entries, .. } => Ok(entries),
                                        _ => anyhow::bail!("目录列表返回了不匹配的结果"),
                                    }
                                }
                            })
                            .await?;
                        let mut first = None;
                        for (index, item) in request.items().iter().enumerate() {
                            let remote = item.path.clone();
                            let asset = if item.directory {
                                request.directory_asset(index, &remote).await?
                            } else {
                                ensure!(
                                    !client.metadata(remote.clone()).await?.is_dir(),
                                    "文件已变为目录，请刷新后重试"
                                );
                                sftp::export_file(&client, &request, index, remote).await?
                            };
                            if first.is_none() {
                                first = Some(asset);
                            }
                        }
                        Ok(Output::ExportReady(
                            request.path.clone(),
                            first.ok_or_else(|| anyhow::anyhow!("拖出列表为空"))?,
                        ))
                    })
                    .await
            }
            other => {
                let client = self.sftp().await?;
                sftp::execute(&client, other).await
            }
        }
    }
    pub(super) async fn exec(&self, command: &str) -> Result<String> {
        tokio::time::timeout(Duration::from_secs(120), async {
            let mut channel = self.session.channel_open_session().await?;
            channel.exec(true, command).await?;
            let mut bytes = Vec::new();
            let mut exit = None;
            let mut acknowledged = false;
            let mut truncated = false;
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Success => acknowledged = true,
                    ChannelMsg::Failure => anyhow::bail!("远端拒绝执行命令"),
                    ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                        let room = (2 * 1024 * 1024_usize).saturating_sub(bytes.len());
                        bytes.extend_from_slice(&data[..data.len().min(room)]);
                        truncated |= data.len() > room;
                    }
                    ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            ensure!(acknowledged, "远端没有确认命令执行");
            ensure!(exit == Some(0), "远端命令失败（状态 {:?}）：{}", exit, text);
            ensure!(!truncated, "远端输出超过 2 MiB，请缩小查询范围");
            Ok(text)
        })
        .await
        .context("SSH 管理命令执行超时")?
    }
}
pub(super) async fn reply(sender: &mpsc::Sender<Reply>, id: u64, result: Result<Output>) {
    let _ = sender
        .send(Reply {
            id,
            result: result.map_err(|e| format!("{e:#}")),
        })
        .await;
}
pub(super) async fn request_reply(
    sender: &mpsc::Sender<Reply>,
    id: u64,
    response: Option<tokio::sync::oneshot::Sender<std::result::Result<Output, String>>>,
    result: Result<Output>,
) {
    if let Some(response) = response {
        let _ = response.send(result.map_err(|e| format!("{e:#}")));
    } else {
        reply(sender, id, result).await;
    }
}
