use super::*;
use std::{mem::ManuallyDrop, os::windows::ffi::OsStrExt, sync::atomic::AtomicUsize};
use windows::Win32::{
    Foundation::POINTL,
    System::{Memory::*, SystemServices::MODIFIERKEYS_FLAGS},
};

fn folder() -> File {
    File::new(
        "目录".into(),
        None,
        || None,
        |_| anyhow::bail!("directories have no contents"),
    )
    .with_directory(true)
}
fn descriptor_format(descriptor: &descriptor::Descriptor) -> FORMATETC {
    FORMATETC {
        cfFormat: descriptor.group_format,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    }
}
fn count(object: &IDataObject, format: &FORMATETC) -> Result<u32> {
    let mut medium = unsafe { object.GetData(format)? };
    let address = unsafe { GlobalLock(medium.u.hGlobal) };
    anyhow::ensure!(!address.is_null(), "invalid descriptor handle");
    let result = unsafe { address.cast::<u32>().read() };
    unsafe {
        let _ = GlobalUnlock(medium.u.hGlobal);
        ReleaseStgMedium(&mut medium);
    }
    Ok(result)
}

#[test]
fn shell_drop_expands_roots_after_release_and_copies_nested_unicode_and_empty_directory()
-> Result<()> {
    let _apartment = Apartment::new()?;
    let destination = tempfile::tempdir()?;
    let source = Arc::new(tempfile::tempdir()?);
    std::fs::write(source.path().join("中文.txt"), b"released source bytes")?;
    let wide: Vec<_> = destination
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)? };
    let target: IDropTarget = unsafe { item.BindToHandler(None, &BHID_SFUIObject)? };
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let preview = vec![folder()];
    let descriptor = descriptor::Descriptor::new(&preview)?;
    let format = descriptor_format(&descriptor);
    let mut loop_format = format;
    loop_format.cfFormat = descriptor.drag_format;
    let cancel = Cancellation::new();
    let object: IDataObject = data::Deferred::with_expansion(
        descriptor,
        preview,
        cancel.clone(),
        Some(Box::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            let path = source.path().join("中文.txt");
            let empty = File::new(
                "目录\\空目录".into(),
                None,
                || None,
                |_| anyhow::bail!("directories have no contents"),
            )
            .with_directory(true);
            Ok(vec![
                folder(),
                empty,
                File::new(
                    "目录\\中文.txt".into(),
                    Some(21),
                    || None,
                    move |_| Ok((path, source)),
                ),
            ])
        })),
    )
    .into();
    assert_eq!(count(&object, &format)?, 1);
    assert_eq!(count(&object, &loop_format)?, 1);
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
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "hover never enumerates remote directories"
    );
    cancel.1.store(true, Ordering::Release);
    assert_eq!(count(&object, &loop_format)?, 0);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "drag-state queries do not expand directories"
    );
    unsafe {
        target.Drop(
            &object,
            MODIFIERKEYS_FLAGS(0),
            POINTL::default(),
            &mut effect,
        )?;
    }
    assert_eq!(effect, DROPEFFECT_COPY);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(count(&object, &format)?, 3);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let output = destination.path().join("目录/中文.txt");
    while !output.is_file() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(std::fs::read(output)?, b"released source bytes");
    assert!(destination.path().join("目录/空目录").is_dir());
    Ok(())
}

#[test]
fn failed_expansion_is_stable_and_never_falls_back_to_an_empty_folder() -> Result<()> {
    let _apartment = Apartment::new()?;
    let files = vec![folder()];
    let descriptor = descriptor::Descriptor::new(&files)?;
    let format = descriptor_format(&descriptor);
    let cancel = Cancellation::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let object: IDataObject = data::Deferred::with_expansion(
        descriptor,
        files,
        cancel.clone(),
        Some(Box::new(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("owned directory listing failure")
        })),
    )
    .into();
    assert_eq!(count(&object, &format)?, 1);
    cancel.1.store(true, Ordering::Release);
    for _ in 0..2 {
        assert!(unsafe { object.GetData(&format) }.is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn hover_cannot_retain_a_stream_with_indices_that_shift_after_directory_expansion() -> Result<()> {
    let _apartment = Apartment::new()?;
    let source = Arc::new(tempfile::tempdir()?);
    let tail = source.path().join("tail.txt");
    std::fs::write(&tail, b"tail")?;
    let preview = vec![
        folder(),
        File::new(
            "tail.txt".into(),
            Some(4),
            || None,
            |_| anyhow::bail!("hover preview must not resolve a file"),
        ),
    ];
    let descriptor = descriptor::Descriptor::new(&preview)?;
    let format = FORMATETC {
        cfFormat: descriptor.contents_format,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: 1,
        tymed: TYMED_ISTREAM.0 as u32,
        ..Default::default()
    };
    let cancel = Cancellation::new();
    let object: IDataObject = data::Deferred::with_expansion(
        descriptor,
        preview,
        cancel.clone(),
        Some(Box::new(move |_| {
            Ok(vec![
                folder(),
                File::new(
                    "目录\\child.txt".into(),
                    Some(0),
                    || None,
                    |_| anyhow::bail!("not requested"),
                ),
                File::new(
                    "tail.txt".into(),
                    Some(4),
                    || None,
                    move |_| Ok((tail, source)),
                ),
            ])
        })),
    )
    .into();
    unsafe {
        object.QueryGetData(&format).ok()?;
    }
    let error = unsafe { object.GetData(&format) }
        .err()
        .expect("hover must defer shifting indices");
    assert_eq!(error.code(), PENDING);
    cancel.1.store(true, Ordering::Release);
    let mut final_format = format;
    final_format.lindex = 2;
    let mut medium = unsafe { object.GetData(&final_format)? };
    let stream = unsafe { ManuallyDrop::take(&mut medium.u.pstm) }.expect("accepted file stream");
    let mut bytes = [0u8; 8];
    let mut read = 0;
    unsafe {
        stream
            .Read(bytes.as_mut_ptr().cast(), 8, Some(&mut read))
            .ok()?;
    }
    assert_eq!(&bytes[..read as usize], b"tail");
    Ok(())
}
