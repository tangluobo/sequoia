//! Pending virtual files never force the source window to wait for remote bytes.
use super::*;
use std::{
    cell::{OnceCell, RefCell},
    mem::ManuallyDrop,
    rc::Rc,
};
use windows::Win32::Foundation::{
    DV_E_FORMATETC, E_INVALIDARG, E_NOTIMPL, OLE_E_ADVISENOTSUPPORTED,
};
pub(super) struct Resolved {
    pub path: std::path::PathBuf,
    pub lease: Arc<dyn Any + Send + Sync>,
}
pub(super) struct PendingFile {
    pub size: Option<u64>,
    ready: Ready,
    resolve: RefCell<Option<Resolve>>,
    inner: OnceCell<Resolved>,
    cancel: Cancellation,
    available: Option<Available>,
}
impl PendingFile {
    pub fn available(&self, offset: u64, count: u32) -> windows::core::Result<u32> {
        if !self.cancel.1.load(Ordering::Acquire) {
            return Err(PENDING.into());
        }
        if self.cancel.cancelled() {
            return Err(windows::Win32::Foundation::E_ABORT.into());
        }
        match &self.available {
            Some(wait) => wait(offset, count, self.cancel.clone()).map_err(|error| {
                if self.cancel.cancelled() {
                    windows::Win32::Foundation::E_ABORT.into()
                } else {
                    windows::core::Error::new(E_NOTIMPL, format!("{error:#}"))
                }
            }),
            None => Ok(count),
        }
    }
    fn try_get(&self) -> Option<&Resolved> {
        if self.inner.get().is_none()
            && let Some((path, lease)) = (self.ready)()
        {
            let _ = self.inner.set(Resolved { path, lease });
        }
        self.inner.get()
    }
    pub fn get(&self) -> windows::core::Result<&Resolved> {
        if !self.cancel.1.load(Ordering::Acquire) {
            return Err(PENDING.into());
        }
        if let Some(resolved) = self.try_get() {
            return Ok(resolved);
        }
        if self.cancel.cancelled() {
            return Err(windows::Win32::Foundation::E_ABORT.into());
        }
        if self.try_get().is_none() {
            let resolve = self
                .resolve
                .borrow_mut()
                .take()
                .ok_or_else(|| windows::core::Error::new(E_NOTIMPL, "拖放数据已释放"))?;
            let (path, lease) = resolve(self.cancel.clone())
                .map_err(|e| windows::core::Error::new(E_NOTIMPL, format!("{e:#}")))?;
            let _ = self.inner.set(Resolved { path, lease });
        }
        Ok(self.inner.get().expect("resolved file"))
    }
}
#[implement(IDataObject, IDataObjectAsyncCapability, Agile = false)]
pub(super) struct Deferred {
    preview: Catalog,
    expanded: OnceCell<Catalog>,
    expand: RefCell<Option<Expand>>,
    expansion_error: OnceCell<String>,
    cancel: Cancellation,
    pub(super) operation: Rc<asynchronous::Operation>,
}
struct Catalog {
    descriptor: descriptor::Descriptor,
    files: Vec<Rc<PendingFile>>,
}
impl Deferred {
    #[cfg(test)]
    pub(super) fn new(
        descriptor: descriptor::Descriptor,
        files: Vec<File>,
        cancel: Cancellation,
    ) -> Self {
        Self::with_expansion(descriptor, files, cancel, None)
    }
    pub(super) fn with_expansion(
        descriptor: descriptor::Descriptor,
        files: Vec<File>,
        cancel: Cancellation,
        expand: Option<Expand>,
    ) -> Self {
        Self {
            preview: Catalog::new(descriptor, files, &cancel),
            expanded: OnceCell::new(),
            expand: RefCell::new(expand),
            expansion_error: OnceCell::new(),
            cancel,
            operation: Rc::new(asynchronous::Operation::default()),
        }
    }
    fn catalog(&self) -> windows::core::Result<&Catalog> {
        if let Some(error) = self.expansion_error.get() {
            return Err(windows::core::Error::new(E_NOTIMPL, error));
        }
        if self.cancel.1.load(Ordering::Acquire)
            && self.expanded.get().is_none()
            && let Some(expand) = self.expand.borrow_mut().take()
        {
            let expanded = expand(self.cancel.clone()).and_then(|files| {
                let descriptor = descriptor::Descriptor::new(&files)?;
                Ok(Catalog::new(descriptor, files, &self.cancel))
            });
            match expanded {
                Ok(catalog) => {
                    let _ = self.expanded.set(catalog);
                }
                Err(error) => {
                    let error = format!("{error:#}");
                    let _ = self.expansion_error.set(error.clone());
                    return Err(windows::core::Error::new(E_NOTIMPL, error));
                }
            }
        }
        Ok(self.expanded.get().unwrap_or(&self.preview))
    }
}
impl Catalog {
    fn new(descriptor: descriptor::Descriptor, files: Vec<File>, cancel: &Cancellation) -> Self {
        Self {
            descriptor,
            files: files
                .into_iter()
                .map(|file| {
                    Rc::new(PendingFile {
                        size: file.size,
                        ready: file.ready,
                        resolve: RefCell::new(Some(file.resolve)),
                        inner: OnceCell::new(),
                        cancel: cancel.clone(),
                        available: file.available,
                    })
                })
                .collect(),
        }
    }
}
fn format(cf: u16, medium: TYMED, index: i32) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index,
        tymed: medium.0 as u32,
        ..Default::default()
    }
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IDataObject_Impl for Deferred_Impl {
    fn GetData(&self, request: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        self.QueryGetData(request).ok()?;
        let request = unsafe { &*request };
        if request.cfFormat == self.preview.descriptor.drag_format {
            return self
                .preview
                .descriptor
                .drag_state(!self.cancel.1.load(Ordering::Acquire));
        }
        if request.cfFormat == self.preview.descriptor.group_format {
            return self.catalog()?.descriptor.data();
        }
        if request.cfFormat == self.preview.descriptor.contents_format {
            // Directory expansion can shift every later index. A stream obtained
            // from the root-only hover preview must never retain a wrong index.
            if !self.cancel.1.load(Ordering::Acquire) && self.expand.borrow().is_some() {
                return Err(PENDING.into());
            }
            let file = self
                .catalog()?
                .files
                .get(request.lindex as usize)
                .ok_or_else(|| windows::core::Error::from(DV_E_FORMATETC))?;
            let stream = stream::lazy(file.clone());
            return Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(Some(stream)),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        // Winit requests CF_HDROP on DragEnter. A pending file must return immediately.
        let resolved = self
            .preview
            .files
            .first()
            .filter(|file| {
                self.preview.files.len() == 1
                    && file.available.is_none()
                    && self.expand.borrow().is_none()
            })
            .and_then(|file| file.try_get())
            .ok_or_else(|| windows::core::Error::from(DV_E_FORMATETC))?;
        unsafe {
            data_object(&resolved.path, resolved.lease.clone())
                .map_err(|e| windows::core::Error::new(E_NOTIMPL, format!("{e:#}")))?
                .GetData(request)
        }
    }
    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn QueryGetData(&self, request: *const FORMATETC) -> HRESULT {
        let Some(request) = (unsafe { request.as_ref() }) else {
            return E_INVALIDARG;
        };
        if request.dwAspect != DVASPECT_CONTENT.0 {
            return DV_E_FORMATETC;
        }
        let files = &self.expanded.get().unwrap_or(&self.preview).files;
        let supported = if request.cfFormat == self.preview.descriptor.group_format
            || request.cfFormat == self.preview.descriptor.drag_format
        {
            request.lindex == -1 && request.tymed & TYMED_HGLOBAL.0 as u32 != 0
        } else if request.cfFormat == self.preview.descriptor.contents_format {
            request.lindex >= 0
                && ((request.lindex as usize) < files.len()
                    || (self.expand.borrow().is_some() && request.lindex < 4096))
                && request.tymed & TYMED_ISTREAM.0 as u32 != 0
        } else {
            request.cfFormat == 15
                && request.lindex == -1
                && request.tymed & TYMED_HGLOBAL.0 as u32 != 0
                && self.expand.borrow().is_none()
                && files.len() == 1
                && files[0].available.is_none()
                && files[0].try_get().is_some()
        };
        if supported {
            HRESULT(0)
        } else {
            DV_E_FORMATETC
        }
    }
    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
        E_NOTIMPL
    }
    fn SetData(
        &self,
        _: *const FORMATETC,
        _: *const STGMEDIUM,
        _: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe {
            SHCreateStdEnumFmtEtc(&[
                format(self.preview.descriptor.group_format, TYMED_HGLOBAL, -1),
                format(self.preview.descriptor.contents_format, TYMED_ISTREAM, 0),
                format(self.preview.descriptor.drag_format, TYMED_HGLOBAL, -1),
            ])
        }
    }
    fn DAdvise(
        &self,
        _: *const FORMATETC,
        _: u32,
        _: Ref<'_, IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn DUnadvise(&self, _: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    #[test]
    fn shell_folder_drop_target_materializes_deferred_unicode_and_empty_files() -> Result<()> {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::{Foundation::POINTL, System::SystemServices::MODIFIERKEYS_FLAGS};
        let _apartment = Apartment::new()?;
        let destination = tempfile::tempdir()?;
        let wide: Vec<_> = destination
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)? };
        let target: IDropTarget = unsafe { item.BindToHandler(None, &BHID_SFUIObject)? };
        for (name, bytes) in [
            ("实际拖出-中文.txt", b"shell destination bytes".as_slice()),
            ("empty.txt", b"".as_slice()),
        ] {
            let asset = Arc::new(tempfile::tempdir()?);
            let path = asset.path().join(name);
            std::fs::write(&path, bytes)?;
            let files = vec![File::new(
                name.to_owned(),
                Some(bytes.len() as u64),
                || None,
                move |_| {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    Ok((path, asset))
                },
            )];
            let descriptor = descriptor::Descriptor::new(&files)?;
            let cancel = Cancellation::new();
            let object: IDataObject = Deferred::new(descriptor, files, cancel.clone()).into();
            let mut effect = DROPEFFECT_COPY;
            unsafe {
                target.DragEnter(
                    &object,
                    MODIFIERKEYS_FLAGS(1),
                    POINTL::default(),
                    &mut effect,
                )?;
            }
            assert_eq!(
                effect, DROPEFFECT_COPY,
                "Shell must accept the pending file"
            );
            cancel.1.store(true, Ordering::Release);
            unsafe {
                target.Drop(
                    &object,
                    MODIFIERKEYS_FLAGS(0),
                    POINTL::default(),
                    &mut effect,
                )?;
            }
            assert_eq!(effect, DROPEFFECT_COPY);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let output = destination.path().join(name);
            while !output.is_file() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(
                std::fs::read(&output).context("Shell did not create the destination file")?,
                bytes
            );
        }
        Ok(())
    }
    #[test]
    fn deferred_shell_formats_do_not_wait_and_resolved_unicode_file_keeps_lease() -> Result<()> {
        let _apartment = Apartment::new()?;
        let asset = Arc::new(tempfile::tempdir()?);
        let path = asset.path().join("延迟中文.txt");
        let directory = asset.path().to_owned();
        std::fs::write(&path, b"actual deferred bytes")?;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called = calls.clone();
        let files = vec![File::new(
            "延迟中文.txt".into(),
            Some(21),
            || None,
            move |_| {
                called.fetch_add(1, Ordering::Relaxed);
                Ok((path, asset))
            },
        )];
        let descriptor = descriptor::Descriptor::new(&files)?;
        let group = format(descriptor.group_format, TYMED_HGLOBAL, -1);
        let contents = format(descriptor.contents_format, TYMED_ISTREAM, 0);
        let cancel = Cancellation::new();
        let object: IDataObject = Deferred::new(descriptor, files, cancel.clone()).into();
        assert!(
            unsafe { object.GetData(&format(15, TYMED_HGLOBAL, -1)) }.is_err(),
            "source DragEnter must not wait for remote bytes"
        );
        let _formats = unsafe { object.EnumFormatEtc(DATADIR_GET.0 as u32) }?;
        let mut metadata = unsafe { object.GetData(&group) }.context("get file descriptor")?;
        unsafe {
            ReleaseStgMedium(&mut metadata);
        }
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "metadata must not resolve the file"
        );
        let mut medium = unsafe { object.GetData(&contents) }.context("get file contents")?;
        let stream = unsafe { ManuallyDrop::take(&mut medium.u.pstm) }.expect("file stream");
        let mut stat = STATSTG::default();
        unsafe {
            stream.Stat(&mut stat, STATFLAG_NONAME)?;
            stream.Seek(0, STREAM_SEEK_SET, None)?;
        }
        assert_eq!(stat.cbSize, 21);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "GetData/Stat/Seek during hover must not request source bytes"
        );
        let cloned = unsafe { stream.Clone() }.context("clone exported stream")?;
        drop((object, stream));
        assert!(
            directory.exists(),
            "stream clone must retain staged bytes after data-object release"
        );
        let mut bytes = [0u8; 64];
        let mut read = 0;
        assert_eq!(
            unsafe {
                cloned.Read(
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    Some(&mut read),
                )
            },
            PENDING
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        cancel.1.store(true, Ordering::Release);
        unsafe {
            cloned.Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
        }
        .ok()?;
        assert_eq!(&bytes[..read as usize], b"actual deferred bytes");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        drop(cloned);
        assert!(
            !directory.exists(),
            "final stream release cleans the export"
        );
        Ok(())
    }
}
