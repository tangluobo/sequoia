use super::*;
use crate::files::{
    clipboard::Content,
    object_copy::{self, Item, Job, Selection},
};
use std::sync::Arc;
impl Storage {
    pub fn with_copy_context(mut self, context: object_copy::Context) -> Self {
        self.copy_context = context;
        self
    }
    pub(super) fn object_location(&self, path: &str) -> Result<(String, String)> {
        ensure!(
            object_copy::is_object(&self.connection),
            "复制粘贴仅支持对象存储"
        );
        let path = self.key(path)?;
        let (bucket, key) = if self.bucket_root {
            let (bucket, key) = path.split_once('/').unwrap_or((&path, ""));
            ensure!(!bucket.is_empty(), "请先进入一个 Bucket 再粘贴");
            (bucket.to_owned(), key.to_owned())
        } else {
            (self.connection.text("bucket").to_owned(), path)
        };
        let root = self.connection.text("s3Prefix").trim_matches('/');
        let key = match (root.is_empty(), key.is_empty()) {
            (true, _) => key,
            (false, true) => root.into(),
            (false, false) => format!("{root}/{key}"),
        };
        Ok((bucket, key))
    }
    pub(super) async fn copy_objects(
        &self,
        directory: String,
        paths: Vec<(String, bool)>,
    ) -> Result<Output> {
        ensure!(
            !self.bucket_root || !self.key(&directory)?.is_empty(),
            "无法复制Bucket"
        );
        let (bucket, _) = self.object_location(&directory)?;
        let mut items = Vec::new();
        for (path, is_dir) in paths {
            let (selected_bucket, key) = self.object_location(&path)?;
            ensure!(
                selected_bucket == bucket && !key.is_empty(),
                "无法复制Bucket"
            );
            items.push(Item {
                key: if is_dir {
                    format!("{}/", key.trim_end_matches('/'))
                } else {
                    key
                },
                directory: is_dir,
            });
        }
        let selection = Selection {
            connection_id: self.connection.id.clone(),
            kind: self.connection.kind().into(),
            bucket,
            items,
        };
        selection.validate()?;
        let notice = format!("已复制 {} 到剪贴板", selection.description());
        let clipboard = self.copy_context.clipboard.clone();
        tokio::task::spawn_blocking(move || clipboard.write_objects(&selection)).await??;
        Ok(Output::ObjectsStaged(notice))
    }
    pub(super) async fn paste_objects(&self, directory: String, job: Arc<Job>) -> Result<Output> {
        let result = async {
            let clipboard = self.copy_context.clipboard.clone();
            let content = tokio::task::spawn_blocking(move || clipboard.read()).await??;
            let selection = match content {
                Content::Files(files) => {
                    self.object_location(&directory)?;
                    job.finish();
                    return Ok(Output::ClipboardFiles(files));
                }
                Content::Objects(selection) => selection,
            };
            let (bucket, prefix) = self.object_location(&directory)?;
            let lookup = self.copy_context.connections.clone();
            let id = selection.connection_id.clone();
            let source = tokio::task::spawn_blocking(move || lookup.find(&id)).await??;
            let request = object_copy::Request {
                source,
                target: self.connection.clone(),
                selection,
                bucket,
                prefix,
            };
            Ok(Output::ObjectsCopied(
                object_copy::execute(request, job.clone()).await?,
            ))
        }
        .await;
        if let Err(error) = &result {
            job.fail(&format!("{error:#}"));
        }
        result
    }
}
