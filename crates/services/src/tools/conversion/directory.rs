//! Depth-first enumeration retains only open directories, never the file list.
use crate::backup::Control;
use anyhow::{Context as _, Result, ensure};
use std::{
    fs::ReadDir,
    path::{Path, PathBuf},
};
pub(super) struct Files {
    stack: Vec<ReadDir>,
    extension: &'static str,
}
impl Files {
    pub(super) fn new(source: &Path, extension: &'static str) -> Result<Self> {
        Ok(Self {
            stack: vec![std::fs::read_dir(source).context("源目录不存在或无法读取")?],
            extension,
        })
    }
    pub(super) fn next(&mut self, control: &Control) -> Result<Option<PathBuf>> {
        loop {
            control.check()?;
            let Some(directory) = self.stack.last_mut() else {
                return Ok(None);
            };
            let Some(entry) = directory.next() else {
                self.stack.pop();
                continue;
            };
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                ensure!(self.stack.len() < 64, "转换源目录深度超过64层");
                self.stack.push(std::fs::read_dir(path)?);
            } else if (kind.is_file() || kind.is_symlink() && path.is_file())
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(self.extension))
            {
                return Ok(Some(path));
            }
        }
    }
}
