use super::*;
use crate::files::{
    object_copy::Job,
    upload::{Entry, Target},
};
use std::sync::Arc;
impl Target for Storage {
    fn validate(&self, directory: &str) -> Result<()> {
        let key = self.key(directory)?;
        ensure!(
            !self.bucket_root || !key.trim_matches('/').is_empty(),
            "请先进入一个 Bucket 再上传文件"
        );
        Ok(())
    }
    fn object_storage(&self) -> bool {
        crate::files::object_copy::is_object(&self.connection)
    }
    async fn mkdir(&self, remote: &str) -> Result<()> {
        if self.bucket_root {
            let target = self.bucket(remote)?;
            target
                .operator
                .create_dir(&target.directory_key(remote)?)
                .await?;
        } else {
            self.operator
                .create_dir(&self.directory_key(remote)?)
                .await?;
        }
        Ok(())
    }
    async fn write(&self, entry: &Entry, remote: &str, job: &Arc<Job>) -> Result<u64> {
        self.upload_file_progress(entry.local.clone(), remote, Some(job.clone()))
            .await
    }
}
