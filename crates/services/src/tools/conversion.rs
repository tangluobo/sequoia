//! Bounded, cancellable tool conversions shared by the desktop and CLI.
use crate::backup::Control;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug)]
pub struct NoImages;
impl std::fmt::Display for NoImages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("未找到 SVG 文件")
    }
}
impl std::error::Error for NoImages {}
mod directory;
pub(super) mod svg;
#[cfg(test)]
mod tests;
pub(super) mod voc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Images {
        source: PathBuf,
        directory: PathBuf,
        #[serde(default)]
        width: u32,
    },
    Dataset {
        source: PathBuf,
        directory: PathBuf,
    },
}
impl Request {
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Images {
                source, directory, ..
            }
            | Self::Dataset { source, directory } => {
                source.capacity().saturating_add(directory.capacity())
            }
        }
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Counts {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub records: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "counts", rename_all = "snake_case")]
pub enum Report {
    Images(Counts),
    Dataset(Counts),
}
impl Report {
    pub fn message(&self) -> String {
        match self {
            Self::Images(c) if c.failed > 0 => format!(
                "转换完成，有失败项！总计: {}, 成功: {}, 失败: {}",
                c.total, c.success, c.failed
            ),
            Self::Images(c) => format!("转换完成！总计: {}, 成功: {}", c.total, c.success),
            Self::Dataset(_) => "转换完成！".into(),
        }
    }
    pub fn has_errors(&self) -> bool {
        matches!(self, Self::Images(c) if c.failed > 0)
    }
}
pub async fn execute(request: Request, control: Control) -> Result<Report> {
    // Retain the permit inside the blocking worker, including when its actor is
    // dropped. Closing one view must not admit another full-size image before
    // the cancelled rasterizer has released its buffer.
    static PERMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    let permit = tokio::select! {
        biased;
        _ = control.wait_cancelled() => anyhow::bail!("操作已取消"),
        permit = PERMIT.acquire() => permit?,
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        control.check()?;
        match request {
            Request::Images {
                source,
                directory,
                width,
            } => svg::batch(&source, &directory, width, &control).map(Report::Images),
            Request::Dataset { source, directory } => {
                voc::convert(&source, &directory.join("coco_annotations.json"), &control)
                    .map(Report::Dataset)
            }
        }
    })
    .await?
}
