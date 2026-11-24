//! Shared retention policy. Limits bound retained application data, not driver/GPU memory.
pub const DEFAULT_QUERY_ROWS: usize = 1_000;
pub const MAX_QUERY_ROWS: usize = 10_000;
pub const QUERY_BYTES: usize = 8 * 1024 * 1024;
pub const QUERY_RESULT_SETS: usize = 64;
pub const TERMINAL_SCROLLBACK: usize = 1_000;
pub const JSON_HISTORY_ENTRIES: usize = 50;
/// Maximum individual history document; the GUI retains only metadata.
pub const JSON_HISTORY_BYTES: usize = 8 * 1024 * 1024;
