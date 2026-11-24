use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct NativeFile {
    path: PathBuf,
}

impl From<PathBuf> for NativeFile {
    fn from(path: PathBuf) -> Self {
        Self { path }
    }
}

impl egui::DroppedFile for NativeFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.path).map_err(|err| err.to_string())
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub(crate) struct ShellFile(pub(crate) tomato_native_drag::ShellDrop);

#[cfg(target_os = "windows")]
impl egui::DroppedFile for ShellFile {
    fn path(&self) -> &Path {
        Path::new("")
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err("a virtual Shell drop must be materialized before reading".to_owned())
    }

    fn deferred_drop(&self) -> Option<egui::DeferredFileDrop> {
        let source = self.0.clone();
        Some((
            source.retained_bytes(),
            std::sync::Arc::new(move || {
                let (paths, lease) = source.stage().map_err(|error| format!("{error:#}"))?;
                Ok((paths, Box::new(lease)))
            }),
        ))
    }
}
