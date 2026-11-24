//! Use the host's alert sound without creating an audio stream per desktop tab.

#[cfg(windows)]
pub fn play_system_bell() -> bool {
    use windows::Win32::{System::Diagnostics::Debug::MessageBeep, UI::WindowsAndMessaging::MB_OK};
    // SAFETY: MB_OK is a documented style and MessageBeep queues the system sound.
    unsafe { MessageBeep(MB_OK).is_ok() }
}

#[cfg(target_os = "macos")]
pub fn play_system_bell() -> bool {
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        fn NSBeep();
    }
    // SAFETY: NSBeep takes no arguments and is called on the UI thread.
    unsafe { NSBeep() };
    true
}

#[cfg(target_os = "linux")]
pub fn play_system_bell() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
        return false;
    }
    if RUNNING.swap(true, Ordering::AcqRel) {
        return true;
    }
    let worker = std::thread::Builder::new()
        .name("tomato-system-bell".into())
        .spawn(|| {
            if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
                // SAFETY: This worker exclusively owns this connection. A null argument
                // opens the process's default X11 display. XCloseDisplay flushes the bell.
                unsafe {
                    let display = (xlib.XOpenDisplay)(std::ptr::null());
                    if !display.is_null() {
                        (xlib.XBell)(display, 0);
                        (xlib.XCloseDisplay)(display);
                    }
                }
            }
            RUNNING.store(false, Ordering::Release);
        });
    if worker.is_err() {
        RUNNING.store(false, Ordering::Release);
        return false;
    }
    true
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn play_system_bell() -> bool {
    false
}
