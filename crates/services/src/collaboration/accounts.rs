use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub base: String,
    pub email: String,
    #[serde(default = "enabled")]
    pub push: bool,
}
fn enabled() -> bool {
    true
}
pub async fn read(root: &Path) -> Result<Vec<Account>> {
    let path = root.join("accounts.json");
    match tokio::fs::metadata(&path).await {
        Ok(meta) => {
            ensure!(meta.len() < 1024 * 1024, "云账号列表超过 1 MiB");
            Ok(serde_json::from_slice(&tokio::fs::read(path).await?)?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}
pub async fn write(root: &Path, accounts: &[Account]) -> Result<()> {
    ensure!(accounts.len() <= 100, "云服务器最多 100 个账号");
    let bytes = serde_json::to_vec_pretty(accounts)?;
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&root)?;
        let mut f = tempfile::NamedTempFile::new_in(&root)?;
        f.write_all(&bytes)?;
        f.as_file().sync_all()?;
        f.persist(root.join("accounts.json")).map_err(|e| e.error)?;
        Ok::<_, anyhow::Error>(())
    })
    .await?
}
