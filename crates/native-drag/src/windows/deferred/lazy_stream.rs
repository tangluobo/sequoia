//! Hover-time GetData/Stat/Seek only expose metadata; Read/CopyTo resolve after release.
use super::*;
use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_INVALIDARG, E_NOTIMPL};
#[implement(IStream, Agile = false)]
struct Lazy {
    file: Rc<data::PendingFile>,
    position: Cell<u64>,
    inner: OnceCell<IStream>,
}

pub(super) fn new(file: Rc<data::PendingFile>) -> IStream {
    Lazy {
        file,
        position: Cell::new(0),
        inner: OnceCell::new(),
    }
    .into()
}
impl Lazy {
    fn stream(&self) -> windows::core::Result<&IStream> {
        if self.inner.get().is_none() {
            let resolved = self.file.get()?;
            let inner = stream::open(&resolved.path, resolved.lease.clone())?;
            let position = i64::try_from(self.position.get())
                .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
            unsafe {
                inner.Seek(position, STREAM_SEEK_SET, None)?;
            }
            let _ = self.inner.set(inner);
        }
        Ok(self.inner.get().expect("initialized file stream"))
    }
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl ISequentialStream_Impl for Lazy_Impl {
    fn Read(&self, data: *mut std::ffi::c_void, size: u32, read: *mut u32) -> HRESULT {
        if size == 0 {
            if !read.is_null() {
                unsafe {
                    *read = 0;
                }
            }
            return HRESULT(0);
        }
        let available = self.file.available(self.position.get(), size);
        let stream = available.and_then(|count| self.stream().map(|stream| (stream, count)));
        match stream {
            Ok((inner, available)) => {
                let mut count = 0;
                let result = unsafe { inner.Read(data, available, Some(&mut count)) };
                self.position
                    .set(self.position.get().saturating_add(u64::from(count)));
                if !read.is_null() {
                    unsafe {
                        *read = count;
                    }
                }
                if result.is_ok() && count < size {
                    HRESULT(1)
                } else {
                    result
                }
            }
            Err(error) => {
                if !read.is_null() {
                    unsafe {
                        *read = 0;
                    }
                }
                error.code()
            }
        }
    }
    fn Write(&self, _: *const std::ffi::c_void, _: u32, _: *mut u32) -> HRESULT {
        E_ACCESSDENIED
    }
}
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl IStream_Impl for Lazy_Impl {
    fn Seek(
        &self,
        offset: i64,
        origin: STREAM_SEEK,
        position: *mut u64,
    ) -> windows::core::Result<()> {
        let base = match origin {
            STREAM_SEEK_SET => 0,
            STREAM_SEEK_CUR => self.position.get(),
            STREAM_SEEK_END => match self.file.size {
                Some(size) => size,
                None => std::fs::metadata(&self.file.get()?.path)
                    .map_err(|_| windows::core::Error::from(E_NOTIMPL))?
                    .len(),
            },
            _ => return Err(E_INVALIDARG.into()),
        };
        let next = u64::try_from(i128::from(base) + i128::from(offset))
            .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
        if let Some(inner) = self.inner.get() {
            let next = i64::try_from(next).map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
            unsafe {
                inner.Seek(next, STREAM_SEEK_SET, None)?;
            }
        }
        self.position.set(next);
        if !position.is_null() {
            unsafe {
                *position = next;
            }
        }
        Ok(())
    }
    fn SetSize(&self, _: u64) -> windows::core::Result<()> {
        Err(E_ACCESSDENIED.into())
    }
    fn CopyTo(
        &self,
        target: Ref<'_, IStream>,
        count: u64,
        read: *mut u64,
        written: *mut u64,
    ) -> windows::core::Result<()> {
        let target = target.as_ref().ok_or(E_INVALIDARG)?;
        let (mut total_read, mut total_written) = (0, 0);
        let mut buffer = vec![0u8; 64 * 1024];
        let result = (|| {
            while total_read < count {
                let amount = (count - total_read).min(buffer.len() as u64) as u32;
                let mut got = 0;
                self.Read(buffer.as_mut_ptr().cast(), amount, &mut got)
                    .ok()?;
                total_read += u64::from(got);
                if got == 0 {
                    break;
                }
                let mut offset = 0;
                while offset < got {
                    let mut put = 0;
                    unsafe {
                        target.Write(
                            buffer.as_ptr().add(offset as usize).cast(),
                            got - offset,
                            Some(&mut put),
                        )
                    }
                    .ok()?;
                    if put == 0 {
                        return Err(windows::Win32::Foundation::STG_E_MEDIUMFULL.into());
                    }
                    offset += put;
                    total_written += u64::from(put);
                }
            }
            Ok(())
        })();
        unsafe {
            if !read.is_null() {
                *read = total_read;
            }
            if !written.is_null() {
                *written = total_written;
            }
        }
        result
    }
    fn Commit(&self, _: &STGC) -> windows::core::Result<()> {
        Ok(())
    }
    fn Revert(&self) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Stat(&self, stat: *mut STATSTG, _: &STATFLAG) -> windows::core::Result<()> {
        if stat.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let size = match self.file.size {
            Some(size) => size,
            None => std::fs::metadata(&self.file.get()?.path)
                .map_err(|_| windows::core::Error::from(E_NOTIMPL))?
                .len(),
        };
        unsafe {
            *stat = STATSTG {
                r#type: STGTY_STREAM.0 as u32,
                cbSize: size,
                grfMode: STGM_READ,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn Clone(&self) -> windows::core::Result<IStream> {
        Ok(Lazy {
            file: self.file.clone(),
            position: Cell::new(self.position.get()),
            inner: OnceCell::new(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write as _, mem::ManuallyDrop, os::windows::ffi::OsStrExt};

    #[test]
    fn growing_file_stream_reads_committed_prefix_and_tail_with_the_producer_still_open()
    -> Result<()> {
        let _apartment = Apartment::new()?;
        let lease = Arc::new(tempfile::tempdir()?);
        let path = lease.path().join("growing.txt");
        let mut writer = std::fs::File::create(&path)?;
        writer.write_all(b"abc")?;
        writer.flush()?;
        let available = Arc::new(std::sync::atomic::AtomicUsize::new(3));
        let readable = available.clone();
        let files = vec![
            File::new(
                "growing.txt".into(),
                Some(6),
                || None,
                move |_| Ok((path, lease)),
            )
            .with_available(move |offset, count, _| {
                Ok((readable.load(Ordering::Acquire) as u64)
                    .saturating_sub(offset)
                    .min(u64::from(count)) as u32)
            }),
        ];
        let descriptor = descriptor::Descriptor::new(&files)?;
        let contents = descriptor.contents_format;
        let cancel = Cancellation::new();
        cancel.1.store(true, Ordering::Release);
        let object: IDataObject = data::Deferred::new(descriptor, files, cancel).into();
        let format = FORMATETC {
            cfFormat: contents,
            dwAspect: DVASPECT_CONTENT.0,
            lindex: 0,
            tymed: TYMED_ISTREAM.0 as u32,
            ..Default::default()
        };
        let mut medium = unsafe { object.GetData(&format)? };
        let source = unsafe { ManuallyDrop::take(&mut medium.u.pstm) }.expect("source stream");
        let mut bytes = [0u8; 3];
        let mut count = 0;
        unsafe { source.Read(bytes.as_mut_ptr().cast(), 3, Some(&mut count)) }.ok()?;
        assert_eq!(&bytes, b"abc");
        assert_eq!(count, 3);
        let cloned = unsafe { source.Clone()? };
        writer.write_all(b"def")?;
        writer.flush()?;
        available.store(6, Ordering::Release);
        unsafe { cloned.Read(bytes.as_mut_ptr().cast(), 3, Some(&mut count)) }.ok()?;
        assert_eq!(&bytes, b"def");
        assert_eq!(count, 3);
        let destination = tempfile::tempdir()?;
        let output = destination.path().join("copy.txt");
        let wide: Vec<_> = output.as_os_str().encode_wide().chain(Some(0)).collect();
        let target = unsafe {
            SHCreateStreamOnFileEx(
                PCWSTR(wide.as_ptr()),
                (STGM_CREATE | STGM_READWRITE | STGM_SHARE_DENY_NONE).0,
                0,
                true,
                None::<&IStream>,
            )?
        };
        let (mut read, mut written) = (0, 0);
        unsafe {
            source.Seek(0, STREAM_SEEK_SET, None)?;
            source.CopyTo(&target, u64::MAX, Some(&mut read), Some(&mut written))?;
        }
        assert_eq!((read, written), (6, 6));
        drop(target);
        assert_eq!(std::fs::read(output)?, b"abcdef");
        Ok(())
    }
}
