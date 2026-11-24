//! Test peer only: hold the second describe after the tree has finished loading.
use super::*;
use std::path::{Path, PathBuf};

pub(super) struct Gate {
    root: PathBuf,
    descriptions: usize,
    pub(super) held: bool,
    pub(super) released: bool,
}
impl Gate {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            descriptions: 0,
            held: false,
            released: false,
        }
    }
    pub(super) fn before(&mut self, sql: &str) -> Result<Option<PathBuf>> {
        if sql != "DESCRIBE FORMATTED `default`.`items`" || !self.root.join("arm-fields").exists() {
            return Ok(None);
        }
        self.descriptions += 1;
        if self.descriptions != 2 {
            return Ok(None);
        }
        self.held = true;
        std::fs::write(self.root.join("fields-held"), "held")?;
        Ok(Some(self.root.clone()))
    }
}
pub(super) async fn wait(root: &Path) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    while !root.join("release-fields").exists() {
        ensure!(
            tokio::time::Instant::now() < deadline,
            "field response was never released"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Ok(())
}
