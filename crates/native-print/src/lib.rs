//! Windows print dialog and spooler boundary. A printer is owned by one worker.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{Owner, Printer};
