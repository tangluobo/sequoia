use super::*;
use std::ffi::c_void;
// These test-only declarations avoid adding production Win32 dependency features.
#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowStationW(
        name: *const u16,
        flags: u32,
        access: u32,
        security: *const c_void,
    ) -> *mut c_void;
    fn GetProcessWindowStation() -> *mut c_void;
    fn SetProcessWindowStation(station: *mut c_void) -> i32;
    fn CloseWindowStation(station: *mut c_void) -> i32;
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        mode: *const c_void,
        flags: u32,
        access: u32,
        security: *const c_void,
    ) -> *mut c_void;
    fn GetThreadDesktop(thread: u32) -> *mut c_void;
    fn SetThreadDesktop(desktop: *mut c_void) -> i32;
    fn CloseDesktop(desktop: *mut c_void) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

struct PrivateClipboard {
    station: *mut c_void,
    desktop: *mut c_void,
    original_station: *mut c_void,
    original_desktop: *mut c_void,
}
impl PrivateClipboard {
    fn new() -> Self {
        assert_eq!(
            std::env::var("TOMATO_CLIPBOARD_PRIVATE_TEST").as_deref(),
            Ok("child")
        );
        // SAFETY: queries of this fresh, dedicated test child and its current thread.
        let original_station = unsafe { GetProcessWindowStation() };
        // SAFETY: GetCurrentThreadId takes no arguments.
        let thread = unsafe { GetCurrentThreadId() };
        // SAFETY: querying this child test thread's original desktop.
        let original_desktop = unsafe { GetThreadDesktop(thread) };
        // SAFETY: null name creates a new unnamed station; null security uses the
        // child's default DACL. WINSTA_ALL_ACCESS is scoped to that new object.
        let station =
            unsafe { CreateWindowStationW(std::ptr::null(), 0, 0x037f, std::ptr::null()) };
        assert!(
            !station.is_null(),
            "Cannot create isolated clipboard station: {}",
            io::Error::last_os_error()
        );
        // SAFETY: only this disposable child changes its process window station.
        assert_ne!(unsafe { SetProcessWindowStation(station) }, 0);
        let name: Vec<u16> = "TomatoClipboardRegression\0".encode_utf16().collect();
        // SAFETY: nul-terminated name and null optional pointers, in the new station.
        let desktop = unsafe {
            CreateDesktopW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                0x01ff,
                std::ptr::null(),
            )
        };
        assert!(
            !desktop.is_null(),
            "Cannot create isolated desktop: {}",
            io::Error::last_os_error()
        );
        // SAFETY: this fresh child test thread has not created any HWND/hooks yet.
        assert_ne!(unsafe { SetThreadDesktop(desktop) }, 0);
        Self {
            station,
            desktop,
            original_station,
            original_desktop,
        }
    }
}
impl Drop for PrivateClipboard {
    fn drop(&mut self) {
        // SAFETY: the regression drops all owned HWNDs before this guard; restore
        // only this child, then release objects it created, never the originals.
        let _ = unsafe { SetProcessWindowStation(self.original_station) };
        // SAFETY: original desktop belongs to this same child thread.
        let _ = unsafe { SetThreadDesktop(self.original_desktop) };
        // SAFETY: these handles were created by this guard and are no longer active.
        let _ = unsafe { CloseDesktop(self.desktop) };
        // SAFETY: this guard's private station is no longer assigned to the child.
        let _ = unsafe { CloseWindowStation(self.station) };
    }
}

#[test]
fn tomato_rdp_native_file_clipboard_survives_delayed_rendering() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "clipboard::files::windows::native_tests::tomato_private_rdp_file_clipboard_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TOMATO_CLIPBOARD_PRIVATE_TEST", "child")
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "Private file clipboard child: {status}");
            break;
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Private clipboard child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "run only in the child with an isolated window station"]
fn tomato_private_rdp_file_clipboard_child() {
    let _private = PrivateClipboard::new();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "tomato-rdp-file-copy-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("中文文件.txt");
    let expected: Vec<u8> = (0..CHUNK + 83).map(|index| (index % 251) as u8).collect();
    std::fs::write(&path, &expected).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bridge = Bridge::new(tx.into());
    bridge.file_enabled.store(true, Ordering::Release);
    let files = Arc::new(source::local_entries(&[path.to_string_lossy().into_owned()]).unwrap());
    let offer = Arc::new(Offer {
        origin: bridge.id,
        files,
        sequence: AtomicU32::new(0),
    });
    let mut owner = FileOwner::new(bridge.clone());
    owner.publish(offer.clone()).unwrap();
    let before = sequence();
    let format = clipboard_win::register_format("FileGroupDescriptorW")
        .unwrap()
        .get();
    let descriptor: Vec<u8> =
        clipboard_win::get_clipboard(clipboard_win::formats::RawData(format)).unwrap();
    assert!(descriptor.len() > 500);
    let after = sequence();
    eprintln!("Delayed descriptor rendering: sequence {before} -> {after}");
    assert!(
        local_changed(
            &bridge,
            vec![ClipboardFormat::new(ClipboardFormatId::new(format))]
        )
        .is_none(),
        "Rendering our own file offer must not echo or clear it"
    );
    assert!(
        bridge.publication.lock().unwrap().is_none(),
        "A delayed render must not invalidate the file clipboard"
    );

    let (remote, peer) = tests::native_remote_offer(offer.files.clone());
    owner.publish(remote).unwrap();
    read_native_file_contents(&expected);
    eprintln!("Native OLE stream verified: {} bytes", expected.len());
    owner.clear();
    drop(peer);

    let paths = vec![path.to_string_lossy().into_owned()];
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    // SAFETY: create an invisible system-class window in this disposable child's private desktop.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    // SAFETY: paths remains alive until DestroyWindow removes this subclass.
    assert_ne!(
        unsafe { SetWindowSubclass(hwnd, Some(render_file_list), 1, &paths as *const _ as usize) },
        0
    );
    {
        let _clipboard = clipboard_win::Clipboard::new_for(hwnd).unwrap();
        clipboard_win::empty().unwrap();
        // SAFETY: a null data handle registers delayed CF_HDROP; render_file_list supplies its data.
        unsafe { SetClipboardData(15, std::ptr::null_mut()) };
    }
    let before = sequence();
    assert!(
        local_changed(
            &bridge,
            vec![ClipboardFormat::new(ClipboardFormatId::CF_HDROP)]
        )
        .is_none()
    );
    let after = sequence();
    eprintln!("Delayed CF_HDROP rendering: sequence {before} -> {after}");
    let started = std::time::Instant::now();
    loop {
        if let Ok(RdpInputEvent::Clipboard(ClipboardMessage::SendInitiateFileCopy(files))) =
            rx.try_recv()
        {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].name, "中文文件.txt");
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "The actual delayed local file list was never announced to RDP"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let empty = directory.join("空文件.txt");
    let folder = directory.join("空目录");
    std::fs::write(&empty, []).unwrap();
    std::fs::create_dir(&folder).unwrap();
    {
        let _clipboard = clipboard_win::Clipboard::new_for(hwnd).unwrap();
        clipboard_win::empty().unwrap();
        clipboard_win::raw::set_file_list(&[
            path.to_string_lossy().into_owned(),
            empty.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap();
    }
    local_changed(
        &bridge,
        vec![ClipboardFormat::new(ClipboardFormatId::CF_HDROP)],
    );
    let started = std::time::Instant::now();
    loop {
        if let Ok(RdpInputEvent::Clipboard(ClipboardMessage::SendInitiateFileCopy(files))) =
            rx.try_recv()
        {
            assert_eq!(files.len(), 3);
            assert!(
                files
                    .iter()
                    .any(|entry| entry.name == "空文件.txt" && entry.file_size == Some(0))
            );
            assert!(files.iter().any(|entry| entry.name == "空目录"));
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "The native multi-file offer did not arrive"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let missing = directory.join("missing.txt").to_string_lossy().into_owned();
    {
        let _clipboard = clipboard_win::Clipboard::new_for(hwnd).unwrap();
        clipboard_win::empty().unwrap();
        clipboard_win::raw::set_file_list(&[missing]).unwrap();
    }
    local_changed(
        &bridge,
        vec![ClipboardFormat::new(ClipboardFormatId::CF_HDROP)],
    );
    let started = std::time::Instant::now();
    loop {
        if let Ok(RdpInputEvent::Clipboard(ClipboardMessage::SendInitiateCopy(formats))) =
            rx.try_recv()
        {
            assert!(formats.is_empty());
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "A stale local file list must not block CLIPRDR initialization"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let _clipboard = clipboard_win::Clipboard::new_for(hwnd).unwrap();
        clipboard_win::empty().unwrap();
        clipboard_win::raw::set_file_list(&[
            path.to_string_lossy().into_owned(),
            empty.to_string_lossy().into_owned(),
            folder.to_string_lossy().into_owned(),
        ])
        .unwrap();
    }
    local_changed(
        &bridge,
        vec![ClipboardFormat::new(ClipboardFormatId::CF_HDROP)],
    );
    let started = std::time::Instant::now();
    loop {
        if let Ok(RdpInputEvent::Clipboard(ClipboardMessage::SendInitiateFileCopy(files))) =
            rx.try_recv()
        {
            assert_eq!(files.len(), 3);
            assert!(
                files
                    .iter()
                    .any(|file| file.name == "空文件.txt" && file.file_size == Some(0))
            );
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "A valid multi-file copy must recover after a stale file list"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // SAFETY: this thread owns the private window and its subclass context is still alive.
    unsafe { DestroyWindow(hwnd) };
}

fn read_native_file_contents(expected: &[u8]) {
    use ::windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_ISTREAM};
    use ::windows::Win32::System::Ole::{OleGetClipboard, ReleaseStgMedium};
    let format = clipboard_win::register_format("FileContents")
        .unwrap()
        .get();
    // SAFETY: publishing the file owner initialized OLE on this same private STA.
    let data = unsafe { OleGetClipboard() }.unwrap();
    let request = FORMATETC {
        cfFormat: format as u16,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: 0,
        tymed: TYMED_ISTREAM.0 as u32,
    };
    // SAFETY: request is fully initialized and its target-device pointer is null.
    let mut medium = unsafe { data.GetData(&request) }.unwrap();
    let mut received = Vec::new();
    {
        // SAFETY: GetData returned the requested TYMED_ISTREAM variant.
        let stream = unsafe { medium.u.pstm.as_ref() }.unwrap();
        loop {
            let mut bytes = [0u8; 32768];
            let mut count = 0;
            // SAFETY: the destination covers cb bytes; count points to valid writable storage.
            unsafe {
                stream.Read(
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    Some(&mut count),
                )
            }
            .ok()
            .unwrap();
            if count == 0 {
                break;
            }
            received.extend_from_slice(&bytes[..count as usize]);
            assert!(received.len() <= expected.len());
        }
    }
    // SAFETY: release exactly the successful GetData result, after its stream borrow ended.
    unsafe { ReleaseStgMedium(&mut medium) };
    assert_eq!(received, expected);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *const c_void,
    ) -> *mut c_void;
    fn SetClipboardData(format: u32, data: *mut c_void) -> *mut c_void;
    fn DestroyWindow(hwnd: *mut c_void) -> i32;
}
type Subclass = unsafe extern "system" fn(*mut c_void, u32, usize, isize, usize, usize) -> isize;
#[link(name = "comctl32")]
unsafe extern "system" {
    fn SetWindowSubclass(hwnd: *mut c_void, proc: Option<Subclass>, id: usize, data: usize) -> i32;
    fn DefSubclassProc(hwnd: *mut c_void, msg: u32, w: usize, l: isize) -> isize;
}
unsafe extern "system" fn render_file_list(
    hwnd: *mut c_void,
    msg: u32,
    w: usize,
    l: isize,
    _id: usize,
    data: usize,
) -> isize {
    if msg == 0x0305 && w == 15 {
        // SAFETY: this subclass is removed when its window is destroyed, before paths is dropped.
        let paths = unsafe { &*(data as *const Vec<String>) };
        clipboard_win::raw::set_file_list(paths).unwrap();
        return 0;
    }
    // SAFETY: forward this system-supplied message to the original private window procedure.
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}
