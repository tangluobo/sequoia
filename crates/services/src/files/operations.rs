//! Typed requests shared by SSH and native storage adapters.
use crate::terminal::ssh::tools::management::{ManagementAction, Section, Table};
use std::path::PathBuf;
#[derive(Clone, Debug)]
pub enum Operation {
    Batch(Vec<Operation>),
    Query(Section),
    Manage(ManagementAction),
    List(String),
    Mkdir(String),
    Create(String),
    Rename {
        from: String,
        to: String,
    },
    Remove {
        path: String,
        directory: bool,
    },
    ReadText(String),
    ExportFile(super::export::Request),
    ReadImage(super::images::Request),
    ReadDocumentImage {
        document: String,
        source: String,
    },
    ClipboardFiles,
    CopyObjects {
        directory: String,
        paths: Vec<(String, bool)>,
    },
    PasteObjects {
        directory: String,
        job: std::sync::Arc<super::object_copy::Job>,
    },
    PrepareEdit {
        remote: String,
        launch: bool,
    },
    WriteText {
        path: String,
        text: String,
    },
    Upload {
        local: PathBuf,
        remote: String,
    },
    UploadFiles {
        directory: String,
        paths: Vec<PathBuf>,
        deferred: Vec<super::upload::Deferred>,
        job: std::sync::Arc<super::object_copy::Job>,
    },
    Download {
        remote: String,
        local: PathBuf,
    },
}
impl Operation {
    /// Bound queued file payloads independently of the number of requests.
    pub fn queued_bytes(&self) -> usize {
        let dynamic = match self {
            Self::UploadFiles {
                directory,
                paths,
                deferred,
                ..
            } => paths.iter().fold(
                directory
                    .capacity()
                    .saturating_add(paths.capacity() * std::mem::size_of::<PathBuf>())
                    .saturating_add(deferred.iter().map(|drop| drop.retained_bytes).sum()),
                |sum, path| sum.saturating_add(path.capacity()),
            ),
            Self::CopyObjects { directory, paths } => paths.iter().fold(
                directory
                    .capacity()
                    .saturating_add(paths.capacity() * std::mem::size_of::<(String, bool)>()),
                |sum, (p, _)| sum.saturating_add(p.capacity()),
            ),
            Self::PasteObjects { directory, .. } => directory.capacity(),
            Self::ReadImage(request) => request.path.capacity(),
            Self::ReadDocumentImage { document, source } => {
                document.capacity().saturating_add(source.capacity())
            }
            Self::ExportFile(request) => request.queued_bytes(),
            Self::Batch(operations) => operations
                .iter()
                .fold(0usize, |sum, op| sum.saturating_add(op.queued_bytes())),
            Self::List(path)
            | Self::Mkdir(path)
            | Self::Create(path)
            | Self::ReadText(path)
            | Self::Remove { path, .. }
            | Self::PrepareEdit { remote: path, .. } => path.capacity(),
            Self::Rename { from, to } => from.capacity().saturating_add(to.capacity()),
            Self::WriteText { path, text } => path.capacity().saturating_add(text.capacity()),
            Self::Upload { local, remote } | Self::Download { local, remote } => {
                local.capacity().saturating_add(remote.capacity())
            }
            _ => 0,
        };
        std::mem::size_of::<Self>().saturating_add(dynamic)
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
    pub size: u64,
    pub modified: u32,
    pub permissions: u32,
}
impl Entry {
    pub fn modified_text(&self) -> String {
        if self.modified == 0 {
            return String::new();
        }
        chrono::DateTime::from_timestamp(i64::from(self.modified), 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default()
    }
}
#[derive(Debug)]
pub enum Output {
    ExportReady(String, std::sync::Arc<super::export::Asset>),
    Image {
        path: String,
        mode: super::images::Mode,
        asset: super::images::Asset,
    },
    ObjectsCopied(std::sync::Arc<super::object_copy::Snapshot>),
    Uploaded {
        directory: String,
        report: std::sync::Arc<super::object_copy::Snapshot>,
    },
    ObjectsStaged(String),
    Table(Table),
    Directory {
        path: String,
        entries: Vec<Entry>,
    },
    Text {
        path: String,
        text: String,
    },
    Done(String),
    ClipboardFiles(Vec<PathBuf>),
    EditCached {
        remote: String,
        local: PathBuf,
    },
    Inspection {
        id: String,
        value: serde_json::Value,
    },
}
pub struct Request {
    pub id: u64,
    pub operation: Operation,
    pub response: Option<tokio::sync::oneshot::Sender<Result<Output, String>>>,
}
pub struct Reply {
    pub id: u64,
    pub result: Result<Output, String>,
}
