use std::{path::Path, sync::Arc};

#[cfg(target_arch = "wasm32")]
use core::{future::Future, pin::Pin};

/// A deferred file drop and its retained metadata size. The returned owner keeps
/// the materialized paths alive until their consumer finishes.
#[cfg(not(target_arch = "wasm32"))]
pub type DeferredFileDrop = (
    usize,
    Arc<dyn Fn() -> Result<(Vec<std::path::PathBuf>, Box<dyn Send>), String> + Send + Sync>,
);

/// A file dropped into egui.
///
/// The integration owns the concrete file handle, letting egui remain independent of windowing
/// backends and file APIs.
pub trait DroppedFile: core::fmt::Debug {
    /// The path of the dropped file.
    ///
    /// This is an absolute path on native platforms, or empty for a deferred drop. On the web it
    /// contains only the file name because browsers do not expose the file's local path.
    fn path(&self) -> &Path;

    /// Obtain an optional preparation callback. Run it on a blocking worker.
    #[cfg(not(target_arch = "wasm32"))]
    fn deferred_drop(&self) -> Option<DeferredFileDrop> {
        None
    }

    /// Read the file contents.
    ///
    /// This is asynchronous because browsers can only read files asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if the browser cannot read the file.
    #[cfg(target_arch = "wasm32")]
    fn bytes_async(&self) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + '_>>;

    /// Read the file contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    #[cfg(not(target_arch = "wasm32"))]
    fn bytes(&self) -> Result<Vec<u8>, String>;

    /// The browser file handle, if this file was dropped on the web.
    #[cfg(target_arch = "wasm32")]
    fn web_file(&self) -> Option<&web_sys::File> {
        None
    }
}

/// A shared reference to a dropped file.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub type DroppedFileHandle = Arc<dyn DroppedFile + Send + Sync>;

/// A shared reference to a dropped file.
///
/// This is not necessarily `Send + Sync` when wasm threads are enabled, because
/// [`web_sys::File`] is not thread-safe in that configuration.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub type DroppedFileHandle = Arc<dyn DroppedFile>;
