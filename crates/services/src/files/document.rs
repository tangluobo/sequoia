//! Session-scoped document access. Editor requests share the authenticated file actor.
use super::operations::{Operation, Output, Request};
use crate::markdown::resource::{self, Source};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct Client(pub(crate) mpsc::Sender<Request>);
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileDocumentClient")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Read(String),
    Write { path: String, text: String },
    Image { document: String, source: String },
}
impl Action {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::Read(path) => path.capacity(),
            Self::Write { path, text } => path.capacity().saturating_add(text.capacity()),
            Self::Image { document, source } => {
                document.capacity().saturating_add(source.capacity())
            }
        }
    }
}
impl Client {
    pub async fn request(&self, operation: Operation) -> Result<Output> {
        ensure!(
            operation.queued_bytes() <= 9 * 1024 * 1024,
            "文件请求超过内存预算"
        );
        let (send, receive) = oneshot::channel();
        self.0
            .try_send(Request {
                id: 0,
                operation,
                response: Some(send),
            })
            .map_err(|_| anyhow::anyhow!("文件操作队列已满或原连接已关闭"))?;
        receive
            .await
            .context("原文件连接已关闭，请重新打开文档；未保存内容仍保留")?
            .map_err(anyhow::Error::msg)
    }
    pub async fn execute(&self, action: Action) -> Result<crate::commands::Output> {
        use crate::commands::Output as CommandOutput;
        let operation = match action {
            Action::Read(path) => Operation::ReadText(path),
            Action::Write { path, text } => Operation::WriteText { path, text },
            Action::Image { document, source } => match Source::parse(&source)? {
                Source::Relative(_) => Operation::ReadDocumentImage { document, source },
                direct => return Ok(CommandOutput::MarkdownImage(direct.picture().await?)),
            },
        };
        match self.request(operation).await? {
            Output::Text { text, .. } | Output::Done(text) => Ok(CommandOutput::Text(text)),
            Output::Image { asset, .. } => Ok(CommandOutput::MarkdownImage(asset.picture)),
            _ => bail!("文件连接返回了不匹配的文档响应"),
        }
    }
}
pub(crate) fn image_path(document: &str, source: &str) -> Result<String> {
    let drive = document
        .get(..2)
        .filter(|p| p.as_bytes()[0].is_ascii_alphabetic() && p.ends_with(':'));
    let document = drive.map(|_| &document[2..]).unwrap_or(document);
    let decoded = resource::relative_path(source).replace('\\', "/");
    ensure!(
        !decoded.is_empty() && !decoded.contains('\0'),
        "图片路径为空或含有无效字符"
    );
    let mut parts: Vec<_> = if decoded.starts_with('/') {
        Vec::new()
    } else {
        document
            .rsplit_once('/')
            .map(|(dir, _)| dir.split('/').filter(|p| !p.is_empty()).collect())
            .unwrap_or_default()
    };
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    Ok(format!("{}/{}", drive.unwrap_or(""), parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_images_resolve_unicode_relative_paths_without_local_disk_access() -> Result<()> {
        assert_eq!(
            image_path("/notes/readme.md", "../images/%E5%9B%BE.png#part")?,
            "/images/图.png"
        );
        assert_eq!(image_path("/readme.md", "../outside.png")?, "/outside.png");
        assert_eq!(
            image_path("/notes/readme.md", "dir\\..\\图.png")?,
            "/notes/图.png"
        );
        Ok(())
    }
    #[tokio::test]
    async fn document_reply_is_private_and_disconnect_preserves_failure() -> Result<()> {
        let (send, mut receive) = mpsc::channel::<Request>(1);
        let client = Client(send);
        let worker = tokio::spawn(async move {
            let request = receive.recv().await.unwrap();
            request
                .response
                .unwrap()
                .send(Ok(Output::Text {
                    path: "/readme.md".into(),
                    text: "# 完整文档".into(),
                }))
                .unwrap();
        });
        assert!(
            matches!(client.execute(Action::Read("/readme.md".into())).await?, crate::commands::Output::Text(text) if text == "# 完整文档")
        );
        worker.await?;
        assert!(
            client
                .execute(Action::Write {
                    path: "/readme.md".into(),
                    text: "keep draft".into()
                })
                .await
                .is_err()
        );
        Ok(())
    }
}
