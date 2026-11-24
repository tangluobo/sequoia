//! Legacy JavaFX table clipboard: one serialized String plus Unicode text fallback.
use crate::files::object_copy::clipboard_codec;
use anyhow::{Result, ensure};
pub const FORMAT: &str = "application/x-tomato-copy-tables";
pub trait Repository: Send + Sync {
    fn available(&self) -> bool;
    fn read(&self) -> Result<String>;
    fn write(&self, text: &str) -> Result<()>;
}
#[derive(Default)]
pub struct Memory(std::sync::Mutex<String>);
impl Repository for Memory {
    fn available(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|s| s.starts_with("TOMATO_COPY_TABLES\n"))
    }
    fn read(&self) -> Result<String> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("剪贴板状态锁失败"))?
            .clone())
    }
    fn write(&self, text: &str) -> Result<()> {
        ensure!(text.len() <= 1024 * 1024, "表剪贴板超过1 MiB");
        *self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("剪贴板状态锁失败"))? = text.into();
        Ok(())
    }
}
pub struct System;
#[cfg(windows)]
fn format() -> Result<u32> {
    static ID: std::sync::OnceLock<Option<std::num::NonZeroU32>> = std::sync::OnceLock::new();
    ID.get_or_init(|| clipboard_win::register_format(FORMAT))
        .map(std::num::NonZeroU32::get)
        .ok_or_else(|| anyhow::anyhow!("无法注册表剪贴板"))
}
#[cfg(windows)]
impl Repository for System {
    fn available(&self) -> bool {
        if format().is_ok_and(clipboard_win::is_format_avail) {
            return true;
        }
        static CACHE: std::sync::Mutex<(Option<std::num::NonZeroU32>, bool)> =
            std::sync::Mutex::new((None, false));
        let sequence = clipboard_win::seq_num();
        if let Ok(mut cache) = CACHE.lock() {
            if sequence.is_some() && cache.0 == sequence {
                return cache.1;
            }
            if let Ok(text) = self.read() {
                let found = text.starts_with("TOMATO_COPY_TABLES\n");
                *cache = (sequence, found);
                return found;
            }
        }
        false
    }
    fn read(&self) -> Result<String> {
        use clipboard_win::{Clipboard, Getter, formats};
        let _guard = Clipboard::new_attempts(5)?;
        let id = format()?;
        if clipboard_win::is_format_avail(id) {
            let size = clipboard_win::size(id)
                .ok_or_else(|| anyhow::anyhow!("表剪贴板数据为空"))?
                .get();
            ensure!(size <= 1024 * 1024, "表剪贴板超过1 MiB");
            let mut bytes = vec![0; size];
            let count = clipboard_win::raw::get(id, &mut bytes)?;
            bytes.truncate(count);
            return Ok(clipboard_codec::decode(&bytes)?);
        }
        ensure!(
            clipboard_win::size(13).is_some_and(|size| size.get() <= 2 * 1024 * 1024),
            "剪贴板中没有复制的表或文本超过1 MiB"
        );
        let mut text = String::new();
        formats::Unicode.read_clipboard(&mut text)?;
        Ok(text)
    }
    fn write(&self, text: &str) -> Result<()> {
        use clipboard_win::{Clipboard, options::NoClear, raw};
        let bytes = clipboard_codec::encode(text)?;
        let _guard = Clipboard::new_attempts(5)?;
        raw::set(format()?, &bytes)?;
        raw::set_string_with(text, NoClear)?;
        Ok(())
    }
}
#[cfg(not(windows))]
impl Repository for System {
    fn available(&self) -> bool {
        self.read()
            .is_ok_and(|s| s.starts_with("TOMATO_COPY_TABLES\n"))
    }
    fn read(&self) -> Result<String> {
        use clipboard_rs::{Clipboard, ClipboardContext};
        let context = ClipboardContext::new().map_err(|e| anyhow::anyhow!("剪贴板不可用：{e}"))?;
        if let Ok(bytes) = context.get_buffer(FORMAT) {
            return Ok(clipboard_codec::decode(&bytes)?);
        }
        let text = context
            .get_text()
            .map_err(|e| anyhow::anyhow!("剪贴板读取失败：{e}"))?;
        ensure!(text.len() <= 1024 * 1024, "表剪贴板超过1 MiB");
        Ok(text)
    }
    fn write(&self, text: &str) -> Result<()> {
        use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
        let bytes = clipboard_codec::encode(text)?;
        let context = ClipboardContext::new().map_err(|e| anyhow::anyhow!("剪贴板不可用：{e}"))?;
        context
            .set(vec![
                ClipboardContent::Other(FORMAT.into(), bytes),
                ClipboardContent::Text(text.into()),
            ])
            .map_err(|e| anyhow::anyhow!("表复制失败：{e}"))?;
        Ok(())
    }
}

/// Plain text availability for data-cell paste; do not inspect the private table-copy payload.
pub fn text_available() -> bool {
    #[cfg(windows)]
    {
        clipboard_win::is_format_avail(13)
    }
    #[cfg(not(windows))]
    {
        use clipboard_rs::{Clipboard, ClipboardContext};
        ClipboardContext::new()
            .ok()
            .and_then(|context| context.get_text().ok())
            .is_some()
    }
}
