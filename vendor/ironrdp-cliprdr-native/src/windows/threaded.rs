//! Keep delayed clipboard reads and STA teardown away from the application's UI.
use super::{HWND, WinClipboard, mpsc_sync};
use ironrdp_cliprdr::backend::{ClipboardMessageProxy, CliprdrBackendFactory};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicIsize, Ordering},
};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostMessageW, WM_NULL,
};

/// The HWND and OLE callbacks stay on their creating thread. Dropping this
/// handle only requests cancellation; `shutdown` joins from a background owner.
pub struct ThreadedWinClipboard {
    stopping: Arc<AtomicBool>,
    window: Arc<AtomicIsize>,
    thread: Option<JoinHandle<()>>,
}
impl ThreadedWinClipboard {
    pub fn start(
        proxy: impl ClipboardMessageProxy + Send + 'static,
        owner_events: impl FnOnce() -> Box<dyn FnMut()> + Send + 'static,
    ) -> std::io::Result<(Self, Box<dyn CliprdrBackendFactory + Send>)> {
        let stopping = Arc::new(AtomicBool::new(false));
        let window = Arc::new(AtomicIsize::new(0));
        let (ready, receive) = mpsc_sync::sync_channel(1);
        let stop = stopping.clone();
        let hwnd = window.clone();
        let thread = std::thread::Builder::new()
            .name("tomato-rdp-clipboard".into())
            .spawn(move || {
                // Construct the callback (including its OLE/file owner) on this STA.
                let callback = owner_events();
                let owner = WinClipboard::create(proxy, Some(callback), stop.clone());
                let owner = match owner {
                    Ok(owner) => owner,
                    Err(error) => {
                        let _ = ready.send(Err(error.to_string()));
                        return;
                    }
                };
                // The cancel flag is also observed inside delayed WM_RENDERFORMAT.
                hwnd.store(owner.window.0 as isize, Ordering::Release);
                if ready.send(Ok(owner.backend_factory())).is_err() {
                    hwnd.store(0, Ordering::Release);
                    return;
                }
                let mut message = MSG::default();
                while !stop.load(Ordering::Acquire) {
                    // SAFETY: pump only this dedicated thread's window/OLE messages.
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if result.0 <= 0 || stop.load(Ordering::Acquire) {
                        break;
                    }
                    // SAFETY: message came from this same thread's queue.
                    unsafe { DispatchMessageW(&message) };
                }
                hwnd.store(0, Ordering::Release);
                drop(owner);
            })?;
        let handle = Self {
            stopping,
            window,
            thread: Some(thread),
        };
        match receive.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(factory)) => Ok((handle, factory)),
            Ok(Err(error)) => Err(std::io::Error::other(error)),
            Err(error) => Err(std::io::Error::other(error)),
        }
    }
    /// Signal even when the protocol worker is publishing into a full queue.
    pub fn cancellation(&self) -> impl Fn() + Send + Sync + 'static {
        let stopping = self.stopping.clone();
        let window = self.window.clone();
        move || Self::stop_window(&stopping, &window)
    }
    fn stop(&self) {
        Self::stop_window(&self.stopping, &self.window);
    }
    fn stop_window(stopping: &AtomicBool, window: &AtomicIsize) {
        stopping.store(true, Ordering::Release);
        let window = window.load(Ordering::Acquire);
        if window != 0 {
            // SAFETY: PostMessage is asynchronous; a stale, already destroyed
            // HWND simply fails. The owner thread alone destroys the HWND.
            let _ = unsafe {
                PostMessageW(Some(HWND(window as *mut _)), WM_NULL, WPARAM(0), LPARAM(0))
            };
        }
    }
    /// Call on a background thread after stopping the protocol worker.
    pub fn shutdown(mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for ThreadedWinClipboard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
pub(super) fn test_cancel_pending_render_on_its_own_sta() {
    use ironrdp_cliprdr::{
        backend::ClipboardMessage,
        pdu::{ClipboardFormat, ClipboardFormatId},
    };
    use std::time::{Duration, Instant};
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, WM_RENDERFORMAT};
    #[derive(Debug)]
    struct Peer {
        requests: std::sync::mpsc::Sender<()>,
        respond: Arc<AtomicBool>,
        backend: Arc<std::sync::Mutex<Option<Box<dyn ironrdp_cliprdr::backend::CliprdrBackend>>>>,
    }
    impl ClipboardMessageProxy for Peer {
        fn send_clipboard_message(&self, event: ClipboardMessage) {
            if matches!(event, ClipboardMessage::SendInitiatePaste(_)) {
                let _ = self.requests.send(());
                if self.respond.load(Ordering::Acquire) {
                    self.backend
                        .lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .on_format_data_response(
                            ironrdp_cliprdr::pdu::FormatDataResponse::new_unicode_string(
                                "live threaded clipboard",
                            ),
                        );
                }
            }
        }
    }
    // Called only inside the disposable child's private clipboard/window station.
    let (requests, pending) = mpsc_sync::channel();
    let (created, creating) = mpsc_sync::channel();
    let (callback, callbacks) = mpsc_sync::channel();
    let respond = Arc::new(AtomicBool::new(true));
    let bridge = Arc::new(std::sync::Mutex::new(None));
    let caller = std::thread::current().id();
    let (mut owner, factory) = ThreadedWinClipboard::start(
        Peer {
            requests,
            respond: respond.clone(),
            backend: bridge.clone(),
        },
        move || {
            let _ = created.send(std::thread::current().id());
            Box::new(move || {
                let _ = callback.send(());
            })
        },
    )
    .unwrap();
    assert_ne!(
        caller,
        creating.recv_timeout(Duration::from_secs(1)).unwrap()
    );
    let hwnd = HWND(owner.window.load(Ordering::Acquire) as *mut _);
    *bridge.lock().unwrap() = Some(factory.build_cliprdr_backend());
    bridge
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]);
    bridge
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .on_remote_file_list(&[], None);
    callbacks.recv_timeout(Duration::from_secs(1)).unwrap();
    {
        let clipboard = super::os_clipboard::OwnedOsClipboard::new(HWND::default()).unwrap();
        let data = super::clipboard_data_ref::ClipboardDataRef::get(
            &clipboard,
            ClipboardFormatId::CF_UNICODETEXT,
        )
        .unwrap();
        let expected =
            ironrdp_cliprdr::pdu::FormatDataResponse::new_unicode_string("live threaded clipboard");
        assert!(data.data().starts_with(expected.data()));
    }
    pending.recv_timeout(Duration::from_secs(1)).unwrap();
    respond.store(false, Ordering::Release);
    bridge
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]);
    // SAFETY: this hidden HWND is owned by this same isolated test child. Queue
    // a real delayed render; the peer deliberately never supplies its response.
    unsafe { PostMessageW(Some(hwnd), WM_RENDERFORMAT, WPARAM(13), LPARAM(0)) }.unwrap();
    pending.recv_timeout(Duration::from_secs(1)).unwrap();
    let thread = owner.thread.take().unwrap();
    let started = Instant::now();
    drop(owner);
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "UI cancellation blocked"
    );
    thread.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "STA waited for the 10 second remote timeout"
    );
    // SAFETY: query only the test's destroyed hidden HWND.
    assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
    println!(
        "Threaded clipboard: pending remote read cancelled, HWND destroyed on its STA in {:?}",
        started.elapsed()
    );
}
