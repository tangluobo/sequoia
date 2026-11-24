//! Real delayed-rendered clipboard checks in an isolated child/window station.
use super::*;
use ironrdp_cliprdr::backend::ClipboardMessage;
use std::sync::{Mutex, atomic::AtomicUsize};
use std::{
    ffi::c_void,
    time::{Duration, Instant},
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW,
};

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
            std::io::Error::last_os_error()
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
            std::io::Error::last_os_error()
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

struct Remote {
    requests: Arc<AtomicUsize>,
    respond: Arc<AtomicBool>,
    sender: Arc<Mutex<Option<mpsc_sync::SyncSender<BackendEvent>>>>,
}
impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnedClipboardPeer")
    }
}
impl ClipboardMessageProxy for Remote {
    fn send_clipboard_message(&self, message: ClipboardMessage) {
        if matches!(message, ClipboardMessage::SendInitiatePaste(_)) {
            self.requests.fetch_add(1, Ordering::Relaxed);
            if self.respond.load(Ordering::Relaxed) {
                self.sender
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .send(BackendEvent::FormatDataResponse(
                        FormatDataResponse::new_unicode_string("owned clipboard fixture"),
                    ))
                    .unwrap();
            }
        }
    }
}
fn pump(hwnd: HWND) {
    let mut message = MSG::default();
    // SAFETY: process only the child test's hidden HWND on its creating thread.
    while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
        // SAFETY: message was retrieved from this exact HWND's owner queue.
        unsafe { DispatchMessageW(&message) };
    }
}

#[test]
fn tomato_delayed_clipboard_shutdown_and_live_rendering_are_isolated() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "windows::shutdown_tests::tomato_private_clipboard_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TOMATO_CLIPBOARD_PRIVATE_TEST", "child")
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "Private clipboard child failed: {status}");
            break;
        }
        if started.elapsed() >= Duration::from_secs(5) {
            // Cleanup is limited to the child spawned above and is a failure.
            let _ = child.kill();
            let _ = child.wait();
            panic!("Private clipboard child timed out; shutdown is not accepted");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "invoked only in the dedicated child with a private window station"]
fn tomato_private_clipboard_child() {
    let _private = PrivateClipboard::new();
    let requests = Arc::new(AtomicUsize::new(0));
    let respond = Arc::new(AtomicBool::new(true));
    let sender = Arc::new(Mutex::new(None));
    let owner = WinClipboard::new(Remote {
        requests: requests.clone(),
        respond: respond.clone(),
        sender: sender.clone(),
    })
    .unwrap();
    *sender.lock().unwrap() = Some(owner.backend_tx.clone());
    let mut backend = owner.backend_factory().build_cliprdr_backend();
    // Multiple delayed formats remain unanswered when the owner later closes.
    backend.on_remote_copy(&[
        ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT),
        ClipboardFormat::new(ClipboardFormatId::CF_DIB),
        ClipboardFormat::new(ClipboardFormatId::CF_LOCALE),
    ]);
    pump(owner.window);
    let expected = FormatDataResponse::new_unicode_string("owned clipboard fixture");
    {
        let clipboard = os_clipboard::OwnedOsClipboard::new(owner.window).unwrap();
        let data = clipboard_data_ref::ClipboardDataRef::get(
            &clipboard,
            ClipboardFormatId::CF_UNICODETEXT,
        )
        .unwrap();
        assert!(data.data().starts_with(expected.data()));
    }
    assert_eq!(
        requests.load(Ordering::Relaxed),
        1,
        "Live remote clipboard rendering must still work"
    );
    respond.store(false, Ordering::Relaxed);
    let hwnd = owner.window;
    let started = Instant::now();
    drop(owner);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "DestroyWindow waited for an unavailable remote format"
    );
    assert_eq!(
        requests.load(Ordering::Relaxed),
        1,
        "Shutdown must not start remote requests"
    );
    // SAFETY: IsWindow can query a destroyed HWND.
    assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
    {
        let clipboard = os_clipboard::OwnedOsClipboard::new(HWND::default()).unwrap();
        let data = clipboard_data_ref::ClipboardDataRef::get(
            &clipboard,
            ClipboardFormatId::CF_UNICODETEXT,
        )
        .unwrap();
        assert!(
            data.data().starts_with(expected.data()),
            "Already materialized clipboard data was lost"
        );
    }
    println!(
        "Private Windows clipboard: live delayed rendering works; unanswered formats do not delay destruction; materialized text survives; interactive clipboard untouched"
    );
    threaded::test_cancel_pending_render_on_its_own_sta();
}
