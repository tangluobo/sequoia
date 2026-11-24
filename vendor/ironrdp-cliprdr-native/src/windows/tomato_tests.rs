//! Hidden owner window tests: no clipboard contents are changed.
use super::*;
use ironrdp_cliprdr::backend::ClipboardMessage;
use std::{cell::Cell, rc::Rc};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW,
};

#[derive(Debug)]
struct Proxy;
impl ClipboardMessageProxy for Proxy {
    fn send_clipboard_message(&self, _: ClipboardMessage) {}
}
struct Lifetime(Rc<Cell<usize>>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
#[test]
fn tomato_owner_event_runs_without_redraw_and_window_context_is_released() {
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let lifetime = Lifetime(drops.clone());
    let count = calls.clone();
    let owner = WinClipboard::new_with_owner_events(Proxy, move || {
        let _keep_alive = &lifetime;
        count.set(count.get() + 1);
    })
    .unwrap();
    let hwnd = owner.window;
    owner
        .backend_factory()
        .build_cliprdr_backend()
        .on_remote_file_list(&[], None);
    let mut message = MSG::default();
    // SAFETY: pump only this test's hidden HWND on its creating thread.
    while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
        // SAFETY: message was obtained from the owner thread's queue above.
        unsafe { DispatchMessageW(&message) };
    }
    assert!(calls.get() > 0);
    assert_eq!(drops.get(), 0);
    drop(owner);
    assert_eq!(drops.get(), 1);
    // SAFETY: IsWindow accepts an invalidated handle; it only queries validity.
    assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
}
