use anyhow::{Result, anyhow, ensure};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tomato_core::input_share::{Input, KeyCode, Node, Routed, cross};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
mod cursor;

pub enum Update {
    Layout(Vec<Node>),
    Inject(Input),
    Healthy,
    Reset,
    Return,
    Pause,
    Resume,
}
pub struct Capture {
    thread_id: u32,
    tx: mpsc::SyncSender<Update>,
    overflow: Arc<AtomicBool>,
    failure: Arc<AtomicBool>,
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Capture {
    pub fn send(&self, update: Update) {
        if self.tx.try_send(update).is_err() {
            self.overflow.store(true, Ordering::Release);
            self.failure.store(true, Ordering::Release);
        }
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP, WPARAM(0), LPARAM(0));
        }
    }
    pub fn failed(&self) -> bool {
        self.failure.swap(false, Ordering::AcqRel)
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
pub fn screen() -> (i32, i32) {
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let dimensions = bounds();
        if !previous.0.is_null() {
            let _ = SetThreadDpiAwarenessContext(previous);
        }
        dimensions
    }
}
fn bounds() -> (i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}
fn origin() -> (i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        )
    }
}
fn anchor() -> (i32, i32) {
    // The virtual rectangle's center can be a hole between monitors. The primary
    // monitor's center is always a real pixel; return it in virtual coordinates.
    let (ox, oy) = origin();
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) / 2 - ox,
            GetSystemMetrics(SM_CYSCREEN) / 2 - oy,
        )
    }
}
fn keyboard_input(key: KeyCode, down: bool) -> Option<KEYBDINPUT> {
    let (scan, vk, flags) = match key {
        KeyCode::Scan { code, extended } if (1..=255).contains(&code) => (
            code,
            0,
            KEYEVENTF_SCANCODE
                | if extended {
                    KEYEVENTF_EXTENDEDKEY
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
        ),
        KeyCode::Virtual(vk) if (1..=254).contains(&vk) => (0, vk, KEYBD_EVENT_FLAGS(0)),
        _ => return None,
    };
    Some(KEYBDINPUT {
        wScan: scan,
        wVk: VIRTUAL_KEY(vk),
        dwFlags: flags
            | if down {
                KEYBD_EVENT_FLAGS(0)
            } else {
                KEYEVENTF_KEYUP
            },
        ..Default::default()
    })
}
fn key_code(event: &KBDLLHOOKSTRUCT) -> KeyCode {
    if event.scanCode == 0
        || event.vkCode == u32::from(VK_PAUSE.0)
        || event.vkCode == u32::from(VK_SNAPSHOT.0)
    {
        KeyCode::Virtual(event.vkCode as u16)
    } else {
        KeyCode::Scan {
            code: event.scanCode as u16,
            extended: event.flags.0 & LLKHF_EXTENDED.0 != 0,
        }
    }
}
struct State {
    cursor: cursor::Feedback,
    id: String,
    nodes: Vec<Node>,
    target: usize,
    x: i32,
    y: i32,
    out: Box<dyn Fn(Routed) -> bool + Send>,
    healthy: Instant,
    keys: BTreeSet<KeyCode>,
    buttons: BTreeSet<u8>,
    physical: BTreeSet<KeyCode>,
    held_buttons: BTreeSet<u8>,
    enabled: bool,
    failure: Arc<AtomicBool>,
    receiving: bool,
    paused: bool,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
impl State {
    fn remote(&self) -> bool {
        self.nodes.get(self.target).is_some_and(|n| n.id != self.id)
    }
    fn emit(&mut self, input: Input) -> bool {
        let Some(n) = self.nodes.get(self.target) else {
            return false;
        };
        if !(self.out)(Routed {
            target: n.id.clone(),
            input,
        }) {
            self.failure.store(true, Ordering::Release);
            self.reset();
            return false;
        }
        true
    }
    fn reset(&mut self) {
        self.receiving = false;
        self.cursor.clear();
        if self.remote() {
            let _ = (self.out)(Routed {
                target: self.nodes[self.target].id.clone(),
                input: Input::Release,
            });
        }
        self.release();
        self.target = self.nodes.iter().position(|n| n.id == self.id).unwrap_or(0);
        self.physical.clear();
        self.held_buttons.clear();
        self.enabled = false;
        self.receiving = false;
    }
    fn pause(&mut self) {
        self.reset();
        self.paused = true;
    }
    fn release(&mut self) {
        for key in std::mem::take(&mut self.keys) {
            self.inject(Input::Key { key, down: false });
        }
        for button in std::mem::take(&mut self.buttons) {
            self.inject(Input::Button {
                button,
                down: false,
            });
        }
    }
    fn inject(&mut self, event: Input) {
        let mut input = INPUT::default();
        match event {
            Input::Claim | Input::Display { .. } => return,
            Input::Move { x, y } => {
                let (w, h) = bounds();
                if w < 2 || h < 2 {
                    self.failure.store(true, Ordering::Release);
                    return;
                }
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: (i64::from(x.clamp(0, w - 1)) * 65535 / i64::from(w - 1)) as i32,
                    dy: (i64::from(y.clamp(0, h - 1)) * 65535 / i64::from(h - 1)) as i32,
                    dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK,
                    ..Default::default()
                };
            }
            Input::Key { key, down } => {
                let Some(keyboard) = keyboard_input(key, down) else {
                    return;
                };
                if down {
                    self.keys.insert(key);
                } else {
                    self.keys.remove(&key);
                }
                input.r#type = INPUT_KEYBOARD;
                input.Anonymous.ki = keyboard;
            }
            Input::Button { button, down } => {
                let flags = match (button, down) {
                    (0, true) => MOUSEEVENTF_LEFTDOWN,
                    (0, false) => MOUSEEVENTF_LEFTUP,
                    (1, true) => MOUSEEVENTF_RIGHTDOWN,
                    (1, false) => MOUSEEVENTF_RIGHTUP,
                    (2, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (2, false) => MOUSEEVENTF_MIDDLEUP,
                    (3 | 4, true) => MOUSEEVENTF_XDOWN,
                    (3 | 4, false) => MOUSEEVENTF_XUP,
                    _ => return,
                };
                if down {
                    self.buttons.insert(button);
                } else {
                    self.buttons.remove(&button);
                }
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dwFlags: flags,
                    mouseData: if button >= 3 {
                        u32::from(button - 2)
                    } else {
                        0
                    },
                    ..Default::default()
                };
            }
            Input::Wheel { delta, horizontal } => {
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    mouseData: delta as u32,
                    dwFlags: if horizontal {
                        MOUSEEVENTF_HWHEEL
                    } else {
                        MOUSEEVENTF_WHEEL
                    },
                    ..Default::default()
                };
            }
            Input::Release => {
                self.release();
                return;
            }
        }
        // Only our injected inputs are excluded by the hooks; OS privilege rules still apply.
        unsafe {
            if SendInput(&[input], std::mem::size_of::<INPUT>() as i32) != 1 {
                self.failure.store(true, Ordering::Release);
            }
        }
        if self.cursor.sync(self.receiving).is_err() {
            self.failure.store(true, Ordering::Release);
        }
    }
    fn local_activity(&mut self) {
        if self.receiving {
            self.receiving = false;
            self.reset();
            self.enabled = !self.paused;
            if !(self.out)(Routed {
                target: self.id.clone(),
                input: Input::Claim,
            }) {
                self.failure.store(true, Ordering::Release);
            }
        }
    }
    fn move_to(&mut self, x: i32, y: i32) -> bool {
        if !self.enabled || self.nodes.is_empty() {
            return false;
        }
        let local = self.nodes.iter().find(|n| n.id == self.id).unwrap();
        let was_remote = self.remote();
        let (anchor_x, anchor_y) = anchor();
        if was_remote && x == anchor_x && y == anchor_y {
            return true;
        }
        let (nx, ny) = if was_remote {
            (self.x + x - anchor_x, self.y + y - anchor_y)
        } else {
            (
                if x == 0 {
                    -1
                } else if x == local.width - 1 {
                    local.width
                } else {
                    x
                },
                y,
            )
        };
        let (target, nx, ny) = cross(&self.nodes, self.target, nx, ny);
        if target != self.target {
            if was_remote {
                self.emit(Input::Release);
            } else {
                for key in self.physical.clone() {
                    self.inject(Input::Key { key, down: false });
                }
                for button in self.held_buttons.clone() {
                    self.inject(Input::Button {
                        button,
                        down: false,
                    });
                }
            }
            self.target = target;
            if self.remote() {
                if !was_remote && !self.emit(Input::Claim) {
                    return false;
                }
                if !self.emit(Input::Move { x: nx, y: ny }) {
                    return false;
                }
            } else {
                self.inject(Input::Move { x: nx, y: ny });
            }
            for key in self.physical.clone() {
                let e = Input::Key { key, down: true };
                if self.remote() {
                    self.emit(e);
                } else {
                    self.inject(e);
                }
            }
            for button in self.held_buttons.clone() {
                let e = Input::Button { button, down: true };
                if self.remote() {
                    self.emit(e);
                } else {
                    self.inject(e);
                }
            }
        }
        self.x = nx;
        self.y = ny;
        if self.remote() {
            if !self.emit(Input::Move { x: nx, y: ny }) {
                return false;
            }
            unsafe {
                let (ox, oy) = origin();
                let _ = SetCursorPos(ox + anchor_x, oy + anchor_y);
            }
            true
        } else if was_remote {
            let _ = (self.out)(Routed {
                target: self.id.clone(),
                input: Input::Claim,
            });
            self.inject(Input::Move { x: nx, y: ny });
            true
        } else {
            false
        }
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let e = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
        if e.flags & 1 == 0 {
            let consumed = STATE.with(|s| {
                let Ok(mut borrow) = s.try_borrow_mut() else {
                    return false;
                };
                let Some(s) = borrow.as_mut() else {
                    return false;
                };
                s.local_activity();
                if w.0 as u32 == WM_MOUSEMOVE {
                    let (ox, oy) = origin();
                    return s.move_to(e.pt.x - ox, e.pt.y - oy);
                }
                let event = match w.0 as u32 {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => Input::Button {
                        button: 0,
                        down: w.0 as u32 == WM_LBUTTONDOWN,
                    },
                    WM_RBUTTONDOWN | WM_RBUTTONUP => Input::Button {
                        button: 1,
                        down: w.0 as u32 == WM_RBUTTONDOWN,
                    },
                    WM_MBUTTONDOWN | WM_MBUTTONUP => Input::Button {
                        button: 2,
                        down: w.0 as u32 == WM_MBUTTONDOWN,
                    },
                    WM_XBUTTONDOWN | WM_XBUTTONUP => Input::Button {
                        button: 2 + (e.mouseData >> 16) as u8,
                        down: w.0 as u32 == WM_XBUTTONDOWN,
                    },
                    WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Input::Wheel {
                        delta: (e.mouseData >> 16) as i16 as i32,
                        horizontal: w.0 as u32 == WM_MOUSEHWHEEL,
                    },
                    _ => return false,
                };
                if let Input::Button { button, down } = event {
                    if down {
                        s.held_buttons.insert(button);
                    } else {
                        s.held_buttons.remove(&button);
                        if !s.remote() {
                            s.buttons.remove(&button);
                        }
                    }
                }
                s.remote() && s.emit(event)
            });
            if consumed {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let e = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        if e.flags.0 & LLKHF_INJECTED.0 == 0 {
            let consumed = STATE.with(|s| {
                let Ok(mut borrow) = s.try_borrow_mut() else {
                    return false;
                };
                let Some(s) = borrow.as_mut() else {
                    return false;
                };
                s.local_activity();
                // Ctrl+Alt+F12 always restores local control and disables edge switching.
                if e.vkCode == u32::from(VK_F12.0)
                    && e.flags.0 & LLKHF_UP.0 == 0
                    && s.physical
                        .iter()
                        .any(|key| matches!(key, KeyCode::Scan { code: 0x1d, .. }))
                    && s.physical
                        .iter()
                        .any(|key| matches!(key, KeyCode::Scan { code: 0x38, .. }))
                {
                    s.pause();
                    return true;
                }
                let down = e.flags.0 & LLKHF_UP.0 == 0;
                let key = key_code(e);
                if down {
                    s.physical.insert(key);
                } else {
                    s.physical.remove(&key);
                    if !s.remote() {
                        s.keys.remove(&key);
                    }
                }
                s.remote() && s.emit(Input::Key { key, down })
            });
            if consumed {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
pub fn start(id: String, out: Box<dyn Fn(Routed) -> bool + Send>) -> Result<Capture> {
    let (tx, rx) = mpsc::sync_channel(512);
    let overflow = Arc::new(AtomicBool::new(false));
    let overflowed = overflow.clone();
    let failure = Arc::new(AtomicBool::new(false));
    let failed = failure.clone();
    let (stop, stopping) = mpsc::channel();
    let (ready, initialized) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("input-capture".into())
        .spawn(move || {
            STATE.with(|s| {
                *s.borrow_mut() = Some(State {
                    cursor: cursor::Feedback::default(),
                    id,
                    nodes: Vec::new(),
                    target: 0,
                    x: 0,
                    y: 0,
                    out,
                    healthy: Instant::now(),
                    keys: BTreeSet::new(),
                    buttons: BTreeSet::new(),
                    physical: BTreeSet::new(),
                    held_buttons: BTreeSet::new(),
                    enabled: true,
                    failure: failed,
                    receiving: false,
                    paused: false,
                })
            });
            unsafe {
                let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), None, 0);
                let keyboard_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), None, 0);
                let timer = SetTimer(None, 0, 500, None);
                let ok = mouse_hook.is_ok() && keyboard_hook.is_ok() && timer != 0;
                let _ = ready.send((ok, GetCurrentThreadId()));
                if ok {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        if stopping.try_recv().is_ok() {
                            break;
                        }
                        let mut quitting = false;
                        STATE.with(|cell| {
                            let mut borrow = cell.borrow_mut();
                            let s = borrow.as_mut().unwrap();
                            while let Ok(update) = rx.try_recv() {
                                if stopping.try_recv().is_ok() {
                                    quitting = true;
                                    break;
                                }
                                match update {
                                    Update::Layout(nodes) => {
                                        s.reset();
                                        if tomato_core::input_share::validate(&nodes).is_err()
                                            || !nodes.iter().any(|n| n.id == s.id)
                                        {
                                            s.nodes.clear();
                                            continue;
                                        }
                                        s.nodes = nodes;
                                        s.target =
                                            s.nodes.iter().position(|n| n.id == s.id).unwrap_or(0);
                                        s.enabled = !s.paused;
                                    }
                                    Update::Inject(e) => {
                                        s.receiving = !matches!(e, Input::Release | Input::Claim);
                                        s.inject(e);
                                    }
                                    Update::Healthy => s.healthy = Instant::now(),
                                    Update::Reset => s.reset(),
                                    Update::Return => {
                                        s.reset();
                                        s.enabled = !s.paused;
                                    }
                                    Update::Pause => s.pause(),
                                    Update::Resume => {
                                        s.paused = false;
                                        s.reset();
                                        s.enabled = true;
                                    }
                                }
                            }
                            if quitting {
                                return;
                            }
                            if s.cursor.sync(s.receiving).is_err() {
                                s.failure.store(true, Ordering::Release);
                            }
                            if overflowed.swap(false, Ordering::AcqRel)
                                || s.healthy.elapsed() > Duration::from_secs(3)
                            {
                                while rx.try_recv().is_ok() {}
                                s.reset();
                                s.paused = true;
                            }
                            if let Some(local) = s.nodes.iter().find(|n| n.id == s.id) {
                                let (width, height) = bounds();
                                if (width, height) != (local.width, local.height) {
                                    s.reset();
                                    let _ = (s.out)(Routed {
                                        target: s.id.clone(),
                                        input: Input::Display { width, height },
                                    });
                                }
                            }
                        });
                        if quitting {
                            break;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                STATE.with(|s| {
                    if let Some(s) = s.borrow_mut().as_mut() {
                        s.reset();
                    }
                });
                if let Ok(h) = mouse_hook {
                    let _ = UnhookWindowsHookEx(h);
                }
                if let Ok(h) = keyboard_hook {
                    let _ = UnhookWindowsHookEx(h);
                }
                if timer != 0 {
                    let _ = KillTimer(None, timer);
                }
            }
            STATE.with(|s| *s.borrow_mut() = None);
        })?;
    let (ok, thread_id) = initialized
        .recv()
        .map_err(|_| anyhow!("输入线程启动失败"))?;
    ensure!(ok, "无法安装系统键鼠钩子");
    Ok(Capture {
        thread_id,
        tx,
        overflow,
        failure,
        stop,
        thread: Some(thread),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_codes_preserve_extended_scans_and_special_virtual_keys() {
        let scan = KeyCode::Scan {
            code: 0x1d,
            extended: true,
        };
        let input = keyboard_input(scan, false).unwrap();
        assert_eq!(input.wScan, 0x1d);
        assert_eq!(input.wVk.0, 0);
        assert_eq!(
            input.dwFlags,
            KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
        );
        for (vk, scan_code) in [(VK_VOLUME_UP, 0), (VK_PAUSE, 0x45), (VK_SNAPSHOT, 0x37)] {
            let event = KBDLLHOOKSTRUCT {
                vkCode: u32::from(vk.0),
                scanCode: scan_code,
                ..Default::default()
            };
            let key = key_code(&event);
            assert_eq!(key, KeyCode::Virtual(vk.0));
            let input = keyboard_input(key, false).unwrap();
            assert_eq!(input.wVk, vk);
            assert_eq!(input.wScan, 0);
            assert_eq!(input.dwFlags, KEYEVENTF_KEYUP);
        }
        assert!(keyboard_input(KeyCode::Virtual(0), true).is_none());
        assert!(
            keyboard_input(
                KeyCode::Scan {
                    code: 256,
                    extended: false
                },
                true
            )
            .is_none()
        );
    }
}
