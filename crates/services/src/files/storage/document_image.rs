//! Resolve image references in the document's bucket while retaining its session route.
use super::*;
use crate::{
    files::{document, images, object_copy},
    markdown::{self, resource},
};

impl Storage {
    pub(super) async fn read_document_image(&self, document: &str, source: &str) -> Result<Output> {
        if !object_copy::is_object(&self.connection) {
            let path = document::image_path(document, source)?;
            return self
                .read_image(images::Request::new(path, images::Mode::Document))
                .await;
        }
        let relative = resource::relative_path(source);
        ensure!(
            !relative.is_empty() && !relative.contains('\0'),
            "图片路径为空或含有无效字符"
        );
        let document = self.key(document)?;
        let (bucket, key) = if self.bucket_root {
            document.split_once('/').context("文档不在 Bucket 中")?
        } else {
            (self.connection.text("bucket"), document.as_str())
        };
        let prefix = self.connection.text("s3Prefix").trim_matches('/');
        let document = format!("{prefix}/{key}");
        let key = markdown::remote_resource_key(&document, source);
        ensure!(!key.is_empty(), "图片地址指向 Bucket 根目录");
        let request = images::Request::new(key.clone(), images::Mode::Document);
        if prefix.is_empty() && !self.bucket_root {
            return read(&self.operator, &key, request).await;
        }
        let mut connection = self.connection.clone();
        connection.set("bucket", bucket);
        connection.set("s3Prefix", "");
        connection.set("useSshTunnel", false);
        let mut root = Self::new(&connection)?;
        root.route = self.route.clone();
        root.apply_route();
        read(&root.operator, &key, request).await
    }

    pub(super) async fn read_image(&self, request: images::Request) -> Result<Output> {
        let key = self.key(&request.path)?;
        read(&self.operator, &key, request).await
    }
}

async fn read(operator: &Operator, key: &str, request: images::Request) -> Result<Output> {
    let cancel = request.cancel.clone();
    let read = async {
        let permit = request.permit().await?;
        let size = operator.stat(key).await?.content_length();
        ensure!(size <= images::MAX_BYTES as u64, "图片超过 8 MiB");
        let mut reader = operator
            .reader_with(key)
            .concurrent(1)
            .chunk(64 * 1024)
            .await?
            .into_futures_async_read(..)
            .await?;
        let mut bytes = Vec::with_capacity(size as usize);
        let mut chunk = vec![0; 64 * 1024];
        loop {
            let count = reader.read(&mut chunk).await?;
            if count == 0 {
                break;
            }
            images::append(&mut bytes, &chunk[..count])?;
        }
        let path = request.path.clone();
        let mode = request.mode;
        let asset = images::decode(request, bytes, permit).await?;
        Ok(Output::Image { path, mode, asset })
    };
    tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("图片加载已取消"), result = read => result }
}
