//! File-backed COM streams retain the export lease through reads and stream clones.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL};
pub(super) fn lazy(file: std::rc::Rc<data::PendingFile>) -> IStream {
    super::lazy_stream::new(file)
}
#[implement(IStream, Agile = false)]
struct Leased {
    inner: IStream,
    lease: Arc<dyn Any + Send + Sync>,
    path: std::path::PathBuf,
}
pub(super) fn open(
    path: &Path,
    lease: Arc<dyn Any + Send + Sync>,
) -> windows::core::Result<IStream> {
    Ok(Leased {
        inner: raw(path)?,
        lease,
        path: path.to_owned(),
    }
    .into())
}
fn raw(path: &Path) -> windows::core::Result<IStream> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let inner = unsafe {
        SHCreateStreamOnFileEx(
            PCWSTR(path.as_ptr()),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None::<&IStream>,
        )?
    };
    Ok(inner)
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl ISequentialStream_Impl for Leased_Impl {
    fn Read(&self, data: *mut std::ffi::c_void, size: u32, read: *mut u32) -> HRESULT {
        unsafe { self.inner.Read(data, size, Some(read)) }
    }
    fn Write(&self, data: *const std::ffi::c_void, size: u32, written: *mut u32) -> HRESULT {
        unsafe { self.inner.Write(data, size, Some(written)) }
    }
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IStream_Impl for Leased_Impl {
    fn Seek(
        &self,
        offset: i64,
        origin: STREAM_SEEK,
        position: *mut u64,
    ) -> windows::core::Result<()> {
        unsafe { self.inner.Seek(offset, origin, Some(position)) }
    }
    fn SetSize(&self, size: u64) -> windows::core::Result<()> {
        unsafe { self.inner.SetSize(size) }
    }
    fn CopyTo(
        &self,
        target: Ref<'_, IStream>,
        count: u64,
        read: *mut u64,
        written: *mut u64,
    ) -> windows::core::Result<()> {
        unsafe {
            self.inner
                .CopyTo(target.as_ref(), count, Some(read), Some(written))
        }
    }
    fn Commit(&self, flags: &STGC) -> windows::core::Result<()> {
        unsafe { self.inner.Commit(*flags) }
    }
    fn Revert(&self) -> windows::core::Result<()> {
        unsafe { self.inner.Revert() }
    }
    fn LockRegion(&self, offset: u64, size: u64, flags: &LOCKTYPE) -> windows::core::Result<()> {
        unsafe { self.inner.LockRegion(offset, size, *flags) }
    }
    fn UnlockRegion(&self, offset: u64, size: u64, flags: u32) -> windows::core::Result<()> {
        unsafe { self.inner.UnlockRegion(offset, size, flags) }
    }
    fn Stat(&self, stat: *mut STATSTG, flags: &STATFLAG) -> windows::core::Result<()> {
        unsafe { self.inner.Stat(stat, *flags) }
    }
    fn Clone(&self) -> windows::core::Result<IStream> {
        let inner = match unsafe { self.inner.Clone() } {
            Ok(inner) => inner,
            Err(error) if error.code() == E_NOTIMPL => {
                let mut position = 0;
                unsafe {
                    self.inner.Seek(0, STREAM_SEEK_CUR, Some(&mut position))?;
                }
                let position = i64::try_from(position)
                    .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
                let inner = raw(&self.path)?;
                unsafe {
                    inner.Seek(position, STREAM_SEEK_SET, None)?;
                }
                inner
            }
            Err(error) => return Err(error),
        };
        Ok(Leased {
            inner,
            lease: self.lease.clone(),
            path: self.path.clone(),
        }
        .into())
    }
}
