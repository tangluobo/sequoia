//! Export feeds a readable disk prefix instead of waiting for download + fsync + rename.
use super::*;
use std::sync::Arc;
use tokio::io::AsyncWriteExt as _;
impl Storage {
    pub(super) async fn export_file(
        &self,
        request: &crate::files::export::Request,
        index: usize,
        remote: &str,
    ) -> Result<Arc<crate::files::export::Asset>> {
        if self.bucket_root {
            return Box::pin(self.bucket(remote)?.export_file(request, index, remote)).await;
        }
        let key = self.key(remote)?;
        ensure!(!self.metadata(&key).await?.is_dir(), "系统拖出仅支持文件");
        let mut output = crate::files::export::Writer::new(remote).await?;
        request.publish(index, output.asset());
        if self.connection.kind() == "FTP" {
            super::ftp::download(&self.connection, &key, &mut output).await?;
        } else {
            let mut reader = self
                .operator
                .reader_with(&key)
                .chunk(64 * 1024)
                .concurrent(1)
                .await?
                .into_futures_async_read(..)
                .await?;
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let count = reader.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count]).await?;
                output.flush().await?;
            }
        }
        output.finish().await
    }
}
