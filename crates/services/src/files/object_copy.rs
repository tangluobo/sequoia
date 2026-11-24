//! Legacy object clipboard, bounded copy planning and cancellable transfer state.
pub(crate) mod clipboard_codec;
mod job;
mod payload;
pub(crate) mod sessions;
use anyhow::Result;
pub use job::{Job, Owner, Phase, Snapshot};
pub use payload::{Item, Selection};
use std::sync::Arc;
use tomato_core::{Connection, Store};
pub trait Connections: Send + Sync {
    fn find(&self, id: &str) -> Result<Connection>;
}
pub struct SavedConnections;
pub struct FileConnections(pub std::path::PathBuf);
impl Connections for SavedConnections {
    fn find(&self, id: &str) -> Result<Connection> {
        if let Some(connection) = sessions::find(id) {
            return Ok(connection);
        }
        FileConnections(Store::default_path()).find(id)
    }
}
impl Connections for FileConnections {
    fn find(&self, id: &str) -> Result<Connection> {
        Store::load(&self.0)?
            .connections
            .into_iter()
            .find(|c| c.id == id && c.kind.is_some())
            .ok_or_else(|| anyhow::anyhow!("找不到源连接配置：{id}"))
    }
}
#[derive(Clone)]
pub struct Context {
    pub clipboard: Arc<dyn super::clipboard::Repository>,
    pub connections: Arc<dyn Connections>,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            clipboard: Arc::new(super::clipboard::System),
            connections: Arc::new(SavedConnections),
        }
    }
}
pub fn is_object(connection: &Connection) -> bool {
    matches!(connection.kind(), "S3" | "ALIYUN_OSS")
        || connection.kind() == "LOCAL_DIRECTORY" && connection.text("directoryType") == "S3"
}
pub struct Request {
    pub source: Connection,
    pub target: Connection,
    pub selection: Selection,
    pub bucket: String,
    pub prefix: String,
}
pub async fn execute(request: Request, job: Arc<Job>) -> Result<Arc<Snapshot>> {
    let result = super::storage::Storage::copy_request(request, &job).await;
    match result {
        Ok(()) => job.finish(),
        Err(error) => {
            job.fail(&format!("{error:#}"));
            return Err(error);
        }
    }
    Ok(job.snapshot())
}
