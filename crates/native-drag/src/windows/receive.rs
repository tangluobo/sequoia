//! Receive Shell drops without copying archive data on the window thread.
use super::*;
use anyhow::{Context, ensure};
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{DRAGDROP_E_NOTREGISTERED, HWND, POINT, POINTL},
    Graphics::Gdi::ScreenToClient,
    System::{
        DataExchange::RegisterClipboardFormatW,
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        SystemServices::MODIFIERKEYS_FLAGS,
        Threading::GetCurrentThreadId,
    },
    UI::WindowsAndMessaging::GetWindowThreadProcessId,
};

const MAX_ITEMS: usize = 1000;
const MAX_ID_LIST: usize = 1024 * 1024;

pub enum DroppedItems {
    Paths(Vec<PathBuf>),
    Shell(ShellDrop),
}

type PendingDrop = ((i32, i32), DroppedItems);
type Inbox = Arc<Mutex<Option<PendingDrop>>>;

pub struct DropInbox(Inbox);

impl DropInbox {
    /// Replace winit's path-only target on its own thread. Winit revokes the
    /// registered target on WM_DESTROY, including when a viewport closes.
    pub fn install(hwnd: isize, notify: impl Fn() + Send + Sync + 'static) -> Result<Option<Self>> {
        let hwnd = HWND(hwnd as *mut _);
        ensure!(
            unsafe { GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId() },
            "drop target must be installed on the window thread"
        );
        let formats = Formats::new()?;
        if let Err(error) = unsafe { RevokeDragDrop(hwnd) } {
            if error.code() == DRAGDROP_E_NOTREGISTERED {
                return Ok(None);
            }
            return Err(error.into());
        }
        let inbox = Arc::new(Mutex::new(None));
        let target: IDropTarget = Target {
            hwnd,
            formats,
            accepts: AtomicBool::new(false),
            inbox: inbox.clone(),
            notify: Box::new(notify),
        }
        .into();
        unsafe { RegisterDragDrop(hwnd, &target)? };
        Ok(Some(Self(inbox)))
    }

    pub fn take(&self) -> Option<PendingDrop> {
        self.0.lock().ok()?.take()
    }
}

struct Formats {
    shell_id_list: u16,
    descriptor: u16,
    descriptor_wide: u16,
}

impl Formats {
    fn new() -> Result<Self> {
        let formats = Self {
            shell_id_list: unsafe { RegisterClipboardFormatW(CFSTR_SHELLIDLIST) } as u16,
            descriptor: unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORA) } as u16,
            descriptor_wide: unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) } as u16,
        };
        ensure!(
            formats.shell_id_list != 0 && formats.descriptor != 0 && formats.descriptor_wide != 0,
            "cannot register Shell drop formats"
        );
        Ok(formats)
    }

    fn capture(&self, data: &IDataObject) -> Result<DroppedItems> {
        let virtual_files = supports(data, self.descriptor_wide) || supports(data, self.descriptor);
        if virtual_files && let Ok(drop) = ShellDrop::capture(data, self.shell_id_list) {
            return Ok(DroppedItems::Shell(drop));
        }
        if let Ok(paths) = paths(data) {
            return Ok(DroppedItems::Paths(paths));
        }
        Ok(DroppedItems::Shell(ShellDrop::capture(
            data,
            self.shell_id_list,
        )?))
    }
}

#[implement(IDropTarget, Agile = false)]
struct Target {
    hwnd: HWND,
    formats: Formats,
    accepts: AtomicBool,
    inbox: Inbox,
    notify: Box<dyn Fn() + Send + Sync>,
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let accepted = data.as_ref().is_some_and(|data| {
            supports(data, CF_HDROP.0) || supports(data, self.formats.shell_id_list)
        });
        self.accepts.store(accepted, Ordering::Relaxed);
        unsafe { set_copy_effect(effect, accepted) };
        Ok(())
    }

    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { set_copy_effect(effect, self.accepts.load(Ordering::Relaxed)) };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepts.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let accepted =
            unsafe { effect.as_ref() }.is_some_and(|effect| effect.0 & DROPEFFECT_COPY.0 != 0);
        let mut position = POINT {
            x: point.x,
            y: point.y,
        };
        let queued = accepted
            && unsafe { ScreenToClient(self.hwnd, &mut position) }.as_bool()
            && data
                .as_ref()
                .and_then(|data| self.formats.capture(data).ok())
                .is_some_and(|items| {
                    self.inbox.lock().is_ok_and(|mut inbox| {
                        *inbox = Some(((position.x, position.y), items));
                        true
                    })
                });
        self.accepts.store(false, Ordering::Relaxed);
        unsafe { set_copy_effect(effect, queued) };
        if queued {
            (self.notify)();
        }
        Ok(())
    }
}

unsafe fn set_copy_effect(effect: *mut DROPEFFECT, accepted: bool) {
    if let Some(effect) = unsafe { effect.as_mut() } {
        *effect = if accepted && effect.0 & DROPEFFECT_COPY.0 != 0 {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        };
    }
}

fn format(id: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: id,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn supports(data: &IDataObject, id: u16) -> bool {
    unsafe { data.QueryGetData(&format(id)) }.is_ok()
}

struct Medium(STGMEDIUM);
impl Medium {
    fn get(data: &IDataObject, id: u16) -> Result<Self> {
        let medium = Self(unsafe { data.GetData(&format(id))? });
        ensure!(
            medium.0.tymed == TYMED_HGLOBAL.0 as u32,
            "unexpected Shell drop medium"
        );
        Ok(medium)
    }
}
impl Drop for Medium {
    fn drop(&mut self) {
        unsafe { ReleaseStgMedium(&mut self.0) };
    }
}

fn paths(data: &IDataObject) -> Result<Vec<PathBuf>> {
    let medium = Medium::get(data, CF_HDROP.0)?;
    let handle = HDROP(unsafe { medium.0.u.hGlobal }.0);
    let count = unsafe { DragQueryFileW(handle, u32::MAX, None) } as usize;
    ensure!((1..=MAX_ITEMS).contains(&count), "invalid file drop count");
    (0..count)
        .map(|i| {
            let length = unsafe { DragQueryFileW(handle, i as u32, None) } as usize;
            ensure!((1..32768).contains(&length), "invalid dropped path length");
            let mut name = vec![0; length + 1];
            ensure!(
                unsafe { DragQueryFileW(handle, i as u32, Some(&mut name)) } as usize == length,
                "cannot read dropped path"
            );
            Ok(PathBuf::from(OsString::from_wide(&name[..length])))
        })
        .collect()
}

/// An owned copy of the Shell item list. No apartment-bound COM object crosses threads.
#[derive(Clone)]
pub struct ShellDrop(Arc<[u8]>);

impl std::fmt::Debug for ShellDrop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellDrop")
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl ShellDrop {
    fn capture(data: &IDataObject, id: u16) -> Result<Self> {
        let medium = Medium::get(data, id)?;
        let handle = unsafe { medium.0.u.hGlobal };
        let size = unsafe { GlobalSize(handle) };
        ensure!(
            (12..=MAX_ID_LIST).contains(&size),
            "invalid Shell drop size"
        );
        let pointer = unsafe { GlobalLock(handle) };
        ensure!(!pointer.is_null(), "cannot lock Shell item list");
        let bytes = unsafe { std::slice::from_raw_parts(pointer.cast(), size) }.into();
        let _ = unsafe { GlobalUnlock(handle) };
        Ok(Self(bytes))
    }

    pub fn retained_bytes(&self) -> usize {
        self.0.len()
    }

    /// Resolve the captured items and copy them into an owned temporary directory.
    /// Call from a blocking worker, never a UI frame or an OLE drop callback.
    pub fn stage(&self) -> Result<(Vec<PathBuf>, tempfile::TempDir)> {
        let _apartment = Apartment::new()?;
        let bytes = &self.0;
        let count = u32::from_le_bytes(bytes[..4].try_into()?) as usize;
        ensure!((1..=MAX_ITEMS).contains(&count), "invalid Shell item count");
        let header = (count + 2) * 4;
        ensure!(header <= bytes.len(), "truncated Shell item list");
        let pidl = |index: usize| -> Result<*const Common::ITEMIDLIST> {
            let slot = 4 + index * 4;
            let offset = u32::from_le_bytes(bytes[slot..slot + 4].try_into()?) as usize;
            ensure!(
                offset >= header && offset < bytes.len(),
                "invalid Shell item offset"
            );
            let mut cursor = offset;
            loop {
                let length = u16::from_le_bytes(
                    bytes
                        .get(cursor..cursor + 2)
                        .context("truncated Shell item identifier")?
                        .try_into()?,
                ) as usize;
                if length == 0 {
                    break;
                }
                ensure!(
                    length >= 2 && length <= bytes.len() - cursor,
                    "invalid Shell item identifier"
                );
                cursor += length;
            }
            Ok(bytes[offset..].as_ptr().cast())
        };
        let parent = pidl(0)?;
        let absolute = (1..=count)
            .map(|index| {
                let child = pidl(index)?;
                let combined = unsafe { ILCombine(Some(parent), Some(child)) };
                ensure!(!combined.is_null(), "cannot resolve Shell item");
                Ok(Pidl(combined))
            })
            .collect::<Result<Vec<_>>>()?;
        let pointers: Vec<_> = absolute.iter().map(|pidl| pidl.0.cast_const()).collect();
        let items = unsafe { SHCreateShellItemArrayFromIDLists(&pointers)? };
        let temporary = tempfile::Builder::new().prefix("tomato-drop-").tempdir()?;
        let path: Vec<_> = temporary
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            let destination: IShellItem = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)?;
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            operation.SetOperationFlags(
                FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR,
            )?;
            operation.CopyItems(&items, &destination)?;
            operation.PerformOperations()?;
            ensure!(
                !operation.GetAnyOperationsAborted()?.as_bool(),
                "Shell file copy was aborted"
            );
        }
        let paths = std::fs::read_dir(temporary.path())?
            .map(|entry| Ok(entry?.path()))
            .collect::<Result<Vec<_>>>()?;
        ensure!(!paths.is_empty(), "Shell did not extract any files");
        Ok((paths, temporary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn windows_zip_shell_drop_stages_selected_file_and_folder_with_owned_lease() -> Result<()> {
        let _apartment = Apartment::new()?;
        let owned = tempfile::tempdir()?;
        let archive = owned.path().join("drag-source.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive)?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("中文.txt", options)?;
        writer.write_all(b"selected ZIP entry")?;
        writer.add_directory("nested/", options)?;
        writer.start_file("nested/empty.txt", options)?;
        writer.finish()?;

        let name: Vec<_> = archive.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut pointer = std::ptr::null_mut();
        unsafe { SHParseDisplayName(PCWSTR(name.as_ptr()), None, &mut pointer, 0, None)? };
        let archive = Pidl(pointer);
        let folder: IShellFolder =
            unsafe { SHGetDesktopFolder()?.BindToObject(archive.0, None::<&IBindCtx>)? };
        let mut children = None;
        unsafe {
            folder
                .EnumObjects(
                    HWND::default(),
                    (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32,
                    &mut children,
                )
                .ok()?
        };
        let children = children.context("no ZIP entries")?;
        let mut selected = [std::ptr::null_mut(); 2];
        let mut count = 0;
        unsafe { children.Next(&mut selected, Some(&mut count)).ok()? };
        let selected: Vec<_> = selected
            .into_iter()
            .take(count as usize)
            .map(Pidl)
            .collect();
        assert_eq!(selected.len(), 2);
        let pointers: Vec<_> = selected.iter().map(|item| item.0.cast_const()).collect();
        let data: IDataObject = unsafe { folder.GetUIObjectOf(HWND::default(), &pointers, None)? };
        let drop = match Formats::new()?.capture(&data)? {
            DroppedItems::Shell(drop) => drop,
            DroppedItems::Paths(_) => anyhow::bail!("ZIP entries require virtual staging"),
        };
        // The worker uses only the captured item list and creates its own STA.
        std::thread::spawn(move || -> Result<()> {
            let (paths, lease) = drop.stage()?;
            assert_eq!(paths.len(), 2);
            let root = lease.path().to_owned();
            assert_eq!(std::fs::read(root.join("中文.txt"))?, b"selected ZIP entry");
            assert_eq!(std::fs::metadata(root.join("nested/empty.txt"))?.len(), 0);
            std::mem::drop(lease);
            assert!(!root.exists());
            Ok(())
        })
        .join()
        .expect("ZIP staging worker panicked")?;
        for pointer in pointers {
            let data: IDataObject =
                unsafe { folder.GetUIObjectOf(HWND::default(), &[pointer], None)? };
            let drop = ShellDrop::capture(&data, Formats::new()?.shell_id_list)?;
            let (paths, _lease) = drop.stage()?;
            assert_eq!(paths.len(), 1);
            let path = &paths[0];
            match path.file_name().and_then(|name| name.to_str()) {
                Some("nested") => assert!(path.join("empty.txt").is_file()),
                Some("中文.txt") => assert_eq!(std::fs::read(path)?, b"selected ZIP entry"),
                name => anyhow::bail!("unexpected staged item {name:?}"),
            }
        }
        Ok(())
    }
}
