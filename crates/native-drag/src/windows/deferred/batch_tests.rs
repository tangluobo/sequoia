use super::*;
use std::{mem::ManuallyDrop, os::windows::ffi::OsStrExt, sync::atomic::AtomicUsize};
use windows::Win32::{
    Foundation::POINTL,
    System::{Memory::*, SystemServices::MODIFIERKEYS_FLAGS},
};
#[test]
fn shell_directory_drop_preserves_nested_files_empty_directories_and_siblings() -> Result<()> {
    let _apartment = Apartment::new()?;
    let source = Arc::new(tempfile::tempdir()?);
    let destination = tempfile::tempdir()?;
    std::fs::create_dir_all(source.path().join("目录/空目录"))?;
    std::fs::write(source.path().join("目录/中文.txt"), b"nested bytes")?;
    std::fs::write(source.path().join("sibling.txt"), b"sibling bytes")?;
    let reads = Arc::new(AtomicUsize::new(0));
    let mut files = Vec::new();
    for (name, directory, size) in [
        ("目录", true, None),
        ("目录\\空目录", true, None),
        ("目录\\中文.txt", false, Some(12)),
        ("sibling.txt", false, Some(13)),
    ] {
        let lease = source.clone();
        let calls = reads.clone();
        files.push(
            File::new(
                name.into(),
                size,
                || None,
                move |control| {
                    anyhow::ensure!(control.1.load(Ordering::Acquire), "read before drop");
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok((lease.path().join(name), lease))
                },
            )
            .with_directory(directory),
        );
    }
    let descriptor = descriptor::Descriptor::new(&files)?;
    let cancel = Cancellation::new();
    let object: IDataObject = data::Deferred::new(descriptor, files, cancel.clone()).into();
    let wide: Vec<_> = destination
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)? };
    let target: IDropTarget = unsafe { item.BindToHandler(None, &BHID_SFUIObject)? };
    let mut effect = DROPEFFECT_COPY;
    unsafe {
        target.DragEnter(
            &object,
            MODIFIERKEYS_FLAGS(1),
            POINTL::default(),
            &mut effect,
        )?;
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
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
    assert!(destination.path().join("目录/空目录").is_dir());
    assert_eq!(
        std::fs::read(destination.path().join("目录/中文.txt"))?,
        b"nested bytes"
    );
    assert_eq!(
        std::fs::read(destination.path().join("sibling.txt"))?,
        b"sibling bytes"
    );
    Ok(())
}
#[test]
fn shell_batch_drop_exposes_every_descriptor_immediately_and_reads_only_after_release() -> Result<()>
{
    let _apartment = Apartment::new()?;
    let destination = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let contents = [
        ("批量一.txt", b"first bytes".as_slice()),
        ("second.txt", b"second bytes".as_slice()),
        ("empty.txt", b"".as_slice()),
    ];
    let mut files = Vec::new();
    for (name, bytes) in contents {
        let lease = Arc::new(tempfile::tempdir()?);
        let path = lease.path().join(name);
        std::fs::write(&path, bytes)?;
        let counter = calls.clone();
        files.push(File::new(
            name.into(),
            Some(bytes.len() as u64),
            || None,
            move |control| {
                anyhow::ensure!(
                    control.1.load(Ordering::Acquire),
                    "read before mouse release"
                );
                counter.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(80));
                Ok((path, lease))
            },
        ));
    }
    let descriptor = descriptor::Descriptor::new(&files)?;
    let group_format = descriptor.group_format;
    let contents_format = descriptor.contents_format;
    let cancel = Cancellation::new();
    let object: IDataObject = data::Deferred::new(descriptor, files, cancel.clone()).into();
    let format = |cf, tymed: TYMED, lindex| FORMATETC {
        cfFormat: cf,
        dwAspect: DVASPECT_CONTENT.0,
        tymed: tymed.0 as u32,
        lindex,
        ..Default::default()
    };
    let mut medium = unsafe { object.GetData(&format(group_format, TYMED_HGLOBAL, -1))? };
    unsafe {
        let address = GlobalLock(medium.u.hGlobal);
        assert!(!address.is_null());
        assert_eq!(*address.cast::<u32>(), 3);
        let descriptors = address
            .cast::<u8>()
            .add(std::mem::offset_of!(FILEGROUPDESCRIPTORW, fgd))
            .cast::<FILEDESCRIPTORW>();
        for (index, (_, bytes)) in contents.iter().enumerate() {
            let entry = &*descriptors.add(index);
            assert_ne!(entry.dwFlags & FD_FILESIZE.0 as u32, 0);
            assert_ne!(entry.dwFlags & FD_PROGRESSUI.0 as u32, 0);
            assert_eq!(entry.nFileSizeLow as usize, bytes.len());
        }
        let _ = GlobalUnlock(medium.u.hGlobal);
        ReleaseStgMedium(&mut medium);
    }
    for (index, (_, bytes)) in contents.iter().enumerate() {
        let mut medium =
            unsafe { object.GetData(&format(contents_format, TYMED_ISTREAM, index as i32))? };
        let stream = unsafe { ManuallyDrop::take(&mut medium.u.pstm) }.expect("virtual stream");
        let mut stat = STATSTG::default();
        unsafe {
            stream.Stat(&mut stat, STATFLAG_NONAME)?;
        }
        assert_eq!(stat.cbSize as usize, bytes.len());
    }
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "hover metadata and stream acquisition must perform no source reads"
    );
    let wide: Vec<_> = destination
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)? };
    let target: IDropTarget = unsafe { item.BindToHandler(None, &BHID_SFUIObject)? };
    let mut effect = DROPEFFECT_COPY;
    unsafe {
        target.DragEnter(
            &object,
            MODIFIERKEYS_FLAGS(1),
            POINTL::default(),
            &mut effect,
        )?;
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
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
    for (name, bytes) in contents {
        assert_eq!(std::fs::read(destination.path().join(name))?, bytes);
    }
    assert!(calls.load(Ordering::Relaxed) >= 2);
    Ok(())
}

#[test]
fn shell_async_copy_returns_before_delayed_content_and_reports_final_completion() -> Result<()> {
    use windows::core::Interface as _;
    let _apartment = Apartment::new()?;
    let destination = tempfile::tempdir()?;
    let asset = Arc::new(tempfile::tempdir()?);
    let path = asset.path().join("异步复制.txt");
    std::fs::write(&path, b"async source bytes")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let files = vec![File::new(
        "异步复制.txt".into(),
        Some(18),
        || None,
        move |_| {
            called.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(700));
            Ok((path, asset))
        },
    )];
    let descriptor = descriptor::Descriptor::new(&files)?;
    let cancel = Cancellation::new();
    let deferred = data::Deferred::new(descriptor, files, cancel.clone());
    let state = deferred.operation.clone();
    let object: IDataObject = deferred.into();
    let capability: IDataObjectAsyncCapability = object.cast()?;
    unsafe {
        capability.SetAsyncMode(true)?;
    }
    let wide: Vec<_> = destination
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)? };
    let target: IDropTarget = unsafe { item.BindToHandler(None, &BHID_SFUIObject)? };
    let mut effect = DROPEFFECT_COPY;
    unsafe {
        target.DragEnter(
            &object,
            MODIFIERKEYS_FLAGS(1),
            POINTL::default(),
            &mut effect,
        )?;
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    cancel.1.store(true, Ordering::Release);
    let started = std::time::Instant::now();
    unsafe {
        target.Drop(
            &object,
            MODIFIERKEYS_FLAGS(0),
            POINTL::default(),
            &mut effect,
        )?;
    }
    assert!(
        started.elapsed() < std::time::Duration::from_millis(600),
        "Explorer UI waited for remote bytes"
    );
    assert!(
        state.running(),
        "Shell must negotiate asynchronous extraction"
    );
    assert!(state.wait(&cancel)?);
    assert_eq!(
        std::fs::read(destination.path().join("异步复制.txt"))?,
        b"async source bytes"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}
