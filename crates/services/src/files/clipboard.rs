//! Safe platform adapter for Explorer's CF_HDROP file clipboard.
use super::object_copy::{Selection, clipboard_codec};
use anyhow::Result;
use std::path::PathBuf;
pub const OBJECT_FORMAT: &str = "application/x-s3-file-copy";
#[derive(Clone, Debug)]
pub enum Content {
    Files(Vec<PathBuf>),
    Objects(Selection),
}
pub trait Repository: Send + Sync {
    fn read(&self) -> Result<Content>;
    fn write_objects(&self, selection: &Selection) -> Result<()>;
}
#[derive(Default)]
pub struct System;
impl Repository for System {
    fn read(&self) -> Result<Content> {
        read_content()
    }
    fn write_objects(&self, selection: &Selection) -> Result<()> {
        let bytes = clipboard_codec::encode(&selection.text()?)?;
        write_content(bytes, selection.description())
    }
}
#[cfg(windows)]
fn format() -> Result<u32> {
    static FORMAT: std::sync::OnceLock<Option<std::num::NonZeroU32>> = std::sync::OnceLock::new();
    FORMAT
        .get_or_init(|| clipboard_win::register_format(OBJECT_FORMAT))
        .map(std::num::NonZeroU32::get)
        .ok_or_else(|| anyhow::anyhow!("无法注册对象复制剪贴板格式"))
}
/// A Windows metadata query only; it never reads file paths or clipboard text.
/// Other platforms leave the action visible and resolve contents in the worker.
pub fn objects_available() -> bool {
    #[cfg(windows)]
    {
        format().is_ok_and(clipboard_win::is_format_avail)
    }
    #[cfg(not(windows))]
    {
        true
    }
}
pub fn files_available() -> bool {
    #[cfg(windows)]
    {
        clipboard_win::is_format_avail(15)
    }
    #[cfg(not(windows))]
    {
        true
    }
}
#[cfg(windows)]
fn read_content() -> Result<Content> {
    use clipboard_win::{Clipboard, Getter, formats};
    let _clipboard =
        Clipboard::new_attempts(5).map_err(|e| anyhow::anyhow!("剪贴板暂不可用：{e}"))?;
    if clipboard_win::is_format_avail(15) {
        anyhow::ensure!(
            clipboard_win::size(15).is_none_or(|s| s.get() <= 8 * 1024 * 1024),
            "文件剪贴板超过8 MiB"
        );
        let mut files = Vec::<PathBuf>::new();
        formats::FileList
            .read_clipboard(&mut files)
            .map_err(|e| anyhow::anyhow!("无法读取文件剪贴板：{e}"))?;
        anyhow::ensure!(files.len() <= 1000, "单次粘贴最多1000项");
        if !files.is_empty() {
            return Ok(Content::Files(files));
        }
    }
    let format = format()?;
    anyhow::ensure!(
        clipboard_win::is_format_avail(format),
        "剪贴板中没有可粘贴的文件或对象"
    );
    let size = clipboard_win::size(format)
        .ok_or_else(|| anyhow::anyhow!("剪贴板对象数据不可用"))?
        .get();
    anyhow::ensure!(
        size <= clipboard_codec::MAX_CLIPBOARD,
        "对象复制剪贴板超过1 MiB"
    );
    let mut bytes = vec![0; size];
    let read = clipboard_win::raw::get(format, &mut bytes)
        .map_err(|e| anyhow::anyhow!("读取对象剪贴板失败：{e}"))?;
    bytes.truncate(read);
    Ok(Content::Objects(Selection::parse(
        &clipboard_codec::decode(&bytes)?,
    )?))
}
#[cfg(windows)]
fn write_content(bytes: Vec<u8>, description: String) -> Result<()> {
    use clipboard_win::{Clipboard, options::NoClear, raw};
    let format = format()?;
    let _clipboard =
        Clipboard::new_attempts(5).map_err(|e| anyhow::anyhow!("剪贴板暂不可用：{e}"))?;
    raw::set(format, &bytes).map_err(|e| anyhow::anyhow!("复制对象失败：{e}"))?;
    raw::set_string_with(&description, NoClear)
        .map_err(|e| anyhow::anyhow!("复制对象说明失败：{e}"))?;
    Ok(())
}
#[cfg(not(windows))]
fn with_context<T>(action: impl FnOnce(&clipboard_rs::ClipboardContext) -> Result<T>) -> Result<T> {
    static CONTEXT: std::sync::OnceLock<std::sync::Mutex<Option<clipboard_rs::ClipboardContext>>> =
        std::sync::OnceLock::new();
    let mut context = CONTEXT
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("剪贴板状态锁失效"))?;
    if context.is_none() {
        *context = Some(
            clipboard_rs::ClipboardContext::new()
                .map_err(|e| anyhow::anyhow!("剪贴板不可用：{e}"))?,
        );
    }
    action(context.as_ref().expect("initialized clipboard"))
}
#[cfg(not(windows))]
fn local_paths(files: Vec<String>) -> Result<Vec<PathBuf>> {
    anyhow::ensure!(files.len() <= 1000, "单次粘贴最多1000项");
    files
        .into_iter()
        .map(|file| {
            let path = if file.starts_with("file:") {
                url::Url::parse(&file)?
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("剪贴板文件URI无效"))?
            } else {
                PathBuf::from(file)
            };
            anyhow::ensure!(path.is_absolute(), "剪贴板文件需要绝对路径");
            Ok(path)
        })
        .collect()
}
#[cfg(not(windows))]
fn read_content() -> Result<Content> {
    use clipboard_rs::{Clipboard, ContentFormat};
    with_context(|context| {
        if context.has(ContentFormat::Files) {
            let files = local_paths(
                context
                    .get_files()
                    .map_err(|e| anyhow::anyhow!("文件剪贴板读取失败：{e}"))?,
            )?;
            if !files.is_empty() {
                return Ok(Content::Files(files));
            }
        }
        let bytes = context
            .get_buffer(OBJECT_FORMAT)
            .map_err(|e| anyhow::anyhow!("剪贴板中没有可粘贴的对象：{e}"))?;
        Ok(Content::Objects(Selection::parse(
            &clipboard_codec::decode(&bytes)?,
        )?))
    })
}
#[cfg(not(windows))]
fn write_content(bytes: Vec<u8>, description: String) -> Result<()> {
    use clipboard_rs::{Clipboard, ClipboardContent};
    with_context(|context| {
        context
            .set(vec![
                ClipboardContent::Other(OBJECT_FORMAT.into(), bytes),
                ClipboardContent::Text(description),
            ])
            .map_err(|e| anyhow::anyhow!("复制对象失败：{e}"))
    })
}
#[cfg(windows)]
pub fn paths() -> Result<Vec<PathBuf>> {
    use clipboard_win::{Clipboard, Getter, formats};
    let _clipboard =
        Clipboard::new_attempts(1).map_err(|e| anyhow::anyhow!("文件剪贴板暂不可用：{e}"))?;
    let mut paths: Vec<PathBuf> = Vec::new();
    anyhow::ensure!(
        clipboard_win::size(15).is_none_or(|s| s.get() <= 8 * 1024 * 1024),
        "文件剪贴板超过8 MiB"
    );
    formats::FileList
        .read_clipboard(&mut paths)
        .map_err(|e| anyhow::anyhow!("剪贴板中没有可上传的文件：{e}"))?;
    anyhow::ensure!(paths.len() <= 1000, "单次粘贴最多 1000 项");
    Ok(paths)
}
#[cfg(not(windows))]
pub fn paths() -> Result<Vec<PathBuf>> {
    use clipboard_rs::Clipboard;
    with_context(|context| {
        local_paths(
            context
                .get_files()
                .map_err(|e| anyhow::anyhow!("文件剪贴板读取失败：{e}"))?,
        )
    })
}
