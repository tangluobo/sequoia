//! Shell-owned drag data with an export lease retained until the final COM release.
use anyhow::Result;
use std::{any::Any, path::Path, sync::Arc};
use windows::{
    Win32::{
        System::{Com::*, Ole::*},
        UI::Shell::*,
    },
    core::{BOOL, HRESULT, PCWSTR, Ref, implement},
};
mod deferred;
mod receive;
pub use deferred::{
    Cancellation, Diagnostics, Drag, File, begin, begin_deferred_files, begin_files,
};
pub use receive::{DropInbox, DroppedItems, ShellDrop};
struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        unsafe {
            OleInitialize(None)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}
struct Pidl(*mut Common::ITEMIDLIST);
impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.cast()));
        }
    }
}
// Delegate on the owning STA; the inner Shell object is not advertised as agile.
#[implement(IDataObject, Agile = false)]
struct LeasedData {
    inner: IDataObject,
    _lease: Arc<dyn Any + Send + Sync>,
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IDataObject_Impl for LeasedData_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        unsafe { self.inner.GetData(format) }
    }
    fn GetDataHere(
        &self,
        format: *const FORMATETC,
        medium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        unsafe { self.inner.GetDataHere(format, medium) }
    }
    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        unsafe { self.inner.QueryGetData(format) }
    }
    fn GetCanonicalFormatEtc(&self, input: *const FORMATETC, output: *mut FORMATETC) -> HRESULT {
        unsafe { self.inner.GetCanonicalFormatEtc(input, output) }
    }
    fn SetData(
        &self,
        format: *const FORMATETC,
        medium: *const STGMEDIUM,
        release: BOOL,
    ) -> windows::core::Result<()> {
        unsafe { self.inner.SetData(format, medium, release.as_bool()) }
    }
    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        unsafe { self.inner.EnumFormatEtc(direction) }
    }
    fn DAdvise(
        &self,
        format: *const FORMATETC,
        flags: u32,
        sink: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        unsafe { self.inner.DAdvise(format, flags, sink.as_ref()) }
    }
    fn DUnadvise(&self, connection: u32) -> windows::core::Result<()> {
        unsafe { self.inner.DUnadvise(connection) }
    }
    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        unsafe { self.inner.EnumDAdvise() }
    }
}
fn data_object(path: &Path, lease: Arc<dyn Any + Send + Sync>) -> Result<IDataObject> {
    use std::os::windows::ffi::OsStrExt;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut pointer = std::ptr::null_mut();
    unsafe {
        SHParseDisplayName(PCWSTR(path.as_ptr()), None, &mut pointer, 0, None)?;
    }
    let pidl = Pidl(pointer);
    // The filesystem Shell folder supplies CF_HDROP; SHCreateDataObject alone
    // only guarantees Shell ID lists. https://learn.microsoft.com/windows/win32/shell/clipboard
    let mut last = std::ptr::null_mut();
    let parent: IShellFolder = unsafe { SHBindToParent(pidl.0, Some(&mut last))? };
    let inner = unsafe {
        parent.GetUIObjectOf(
            windows::Win32::Foundation::HWND::default(),
            &[last.cast_const()],
            None,
        )?
    };
    Ok(LeasedData {
        inner,
        _lease: lease,
    }
    .into())
}
pub fn start(path: &Path, lease: Arc<dyn Any + Send + Sync>) -> Result<()> {
    let _apartment = Apartment::new()?;
    let data = data_object(path, lease)?;
    unsafe {
        SHDoDragDrop(None, &data, None::<&IDropSource>, DROPEFFECT_COPY)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn shell_drag_payload_contains_real_unicode_file_and_retains_temp_lease() -> Result<()> {
        let _apartment = Apartment::new()?;
        let asset = Arc::new(tempfile::tempdir()?);
        let path = asset.path().join("远端文档.txt");
        std::fs::write(&path, "actual staged bytes")?;
        let directory = asset.path().to_owned();
        let data = data_object(&path, asset.clone())?;
        drop(asset);
        assert!(path.exists());
        let format = FORMATETC {
            cfFormat: 15,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        assert!(unsafe { data.QueryGetData(&format) }.is_ok());
        let mut medium = unsafe { data.GetData(&format)? };
        let handle = windows::Win32::UI::Shell::HDROP(unsafe { medium.u.hGlobal }.0);
        let mut name = vec![0u16; 32768];
        let count = unsafe { DragQueryFileW(handle, 0, Some(&mut name)) } as usize;
        unsafe {
            ReleaseStgMedium(&mut medium);
        }
        let dropped_path = PathBuf::from(String::from_utf16(&name[..count])?);
        // Shell expands short path components such as RUNNER~1.
        assert_eq!(dropped_path.canonicalize()?, path.canonicalize()?);
        drop(data);
        assert!(!directory.exists());
        Ok(())
    }
}
