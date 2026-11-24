//! Small OS boundary for input and desktop feedback.
mod bell;
pub use bell::play_system_bell;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
pub use platform::*;
