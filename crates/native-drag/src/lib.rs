//! Narrow Windows COM boundary. Core, services and UI retain `unsafe_code = forbid`.
//! Shell callbacks delegate validated data to the system IDataObject; leases own staged files.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    Cancellation, Diagnostics, Drag, DropInbox, DroppedItems, File, ShellDrop, begin,
    begin_deferred_files, begin_files, start,
};
