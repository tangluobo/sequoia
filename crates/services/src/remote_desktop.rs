//! In-process desktop protocol adapters. The UI owns only the latest decoded frame.
mod adb;
pub use adb::VideoStats as AdbVideoStats;
pub use adb::quality::VideoQuality as AdbVideoQuality;
pub async fn wait_adb_cleanup() {
    adb::scrcpy::wait_cleanup().await;
}
mod cursor;
mod surface;
mod vnc;
mod worker;
use anyhow::{Context, Result, ensure};
use ironrdp_client::{
    config::{ClipboardType, Config, ConfigBuilder, Destination},
    rdp::{RdpClient, RdpInputEvent, RdpOutputEvent},
};
use ironrdp_pdu::input::{
    MousePdu,
    fast_path::{FastPathInputEvent, KeyboardFlags},
    mouse::PointerFlags,
};
use ironrdp_pdu::rdp::client_info::PerformanceFlags;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Wake, Waker},
};
use tokio::sync::mpsc;
use tomato_core::Connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    PasswordRequired,
    Ready,
    Closed,
    Failed(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    /// Pixels are 0x00RRGGBB, independent of host byte order.
    pub pixels: Vec<u32>,
    /// None is a complete canvas; Some carries only that changed rectangle.
    pub region: Option<Region>,
}
/// Opaque RGBA bytes in aligned words. Kept separate from the legacy 0x00RRGGBB frame.
#[derive(Clone)]
pub struct RgbaFrame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u32>,
}
/// An opaque RGBA patch of a retained RDP canvas.
pub struct RgbaUpdate {
    pub frame: RgbaFrame,
    pub region: Option<Region>,
}
/// Immutable pixels retained until a GPU upload finishes. A full canvas is
/// shared with the producer; partial snapshots contain only their rectangle.
pub struct RdpSnapshot {
    pub width: u16,
    pub height: u16,
    pub pixels: Arc<Vec<u32>>,
    pub region: Option<Region>,
}
/// Packed I420 planes for GPU conversion. Only the latest picture is retained.
#[derive(Clone)]
pub struct YuvFrame {
    pub width: u16,
    pub height: u16,
    pub planes: [Vec<u8>; 3],
}
/// One retained cursor shape, independent of remote framebuffer round trips.
pub struct Pointer {
    pub size: [u16; 2],
    pub hotspot: [u16; 2],
    pub rgba: Vec<u8>,
}
#[derive(Clone, Copy, Debug)]
pub enum Input {
    AndroidKey(u16),
    ScanCode {
        code: u8,
        extended: bool,
        down: bool,
    },
    Unicode {
        value: u16,
        down: bool,
    },
    Pointer {
        x: u16,
        y: u16,
        button: Option<u8>,
        down: bool,
        wheel: i16,
    },
    Resize {
        width: u16,
        height: u16,
    },
}
type Notifier = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
struct Shared {
    status: Status,
    frame: Option<Frame>,
    rgba_frame: Option<RgbaFrame>,
    yuv_frame: Option<YuvFrame>,
    adb_yuv_output: bool,
    adb_rgba_output: bool,
    adb_generation: u64,
    adb_video: Option<AdbVideoStats>,
    adb_frame_requested_at: Option<std::time::Instant>,
    adb_frame_request_pending: bool,
    rdp_surface: surface::Surface,
    pointer_hidden: bool,
    cursor_mode: cursor::CursorMode,
    pointer: Option<Arc<Pointer>>,
    pointer_revision: u64,
    desktop_name: String,
    clipboard: Option<String>,
    bell: bool,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            status: Status::Connecting,
            frame: None,
            rgba_frame: None,
            yuv_frame: None,
            adb_yuv_output: false,
            adb_rgba_output: true,
            adb_generation: 0,
            adb_video: None,
            adb_frame_requested_at: None,
            adb_frame_request_pending: false,
            rdp_surface: Default::default(),
            pointer_hidden: false,
            cursor_mode: Default::default(),
            pointer: None,
            pointer_revision: 0,
            desktop_name: String::new(),
            clipboard: None,
            bell: false,
        }
    }
}
#[derive(Default)]
struct ClipboardBridge {
    outgoing: Mutex<ClipboardState>,
    ready: tokio::sync::Notify,
}
#[derive(Default)]
struct ClipboardState {
    pending: Option<(String, bool)>,
    busy: bool,
    waiting_keyup: bool,
}
#[derive(Default)]
struct Authentication {
    reply: Mutex<Option<PasswordReply>>,
    ready: tokio::sync::Notify,
}
enum PasswordReply {
    Provided(zeroize::Zeroizing<String>),
    Cancelled,
}
pub struct DesktopSession {
    adb: Option<adb::pool::Lease>,
    shared: Arc<Mutex<Shared>>,
    input: mpsc::Sender<Input>,
    fast_input: Option<ironrdp_client::rdp::FastPathSender>,
    notifier: Notifier,
    // Completion signals cancellation. RDP socket/codec and clipboard STA
    // resources are released by their workers, independently of GUI paints.
    task: Pin<Box<dyn Future<Output = ()>>>,
    runtime: tokio::runtime::Handle,
    waker: Waker,
    finished: bool,
    vnc: bool,
    clipboard: Arc<ClipboardBridge>,
    authentication: Arc<Authentication>,
}
/// Wait for owned RDP protocol/clipboard workers after their sessions are dropped.
pub async fn wait_rdp_cleanup() {
    worker::wait_cleanup().await;
}
pub use adb::AdbPairing;
struct RepaintWake(Notifier);
impl Wake for RepaintWake {
    fn wake(self: Arc<Self>) {
        wake(&self.0);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        wake(&self.0);
    }
}
impl DesktopSession {
    pub fn start(connection: &Connection, runtime: &tokio::runtime::Handle) -> Result<Self> {
        Self::start_configured(connection, runtime, None)
    }
    /// Explicit jump/known-host paths for embedders and isolated protocol fixtures.
    pub fn start_via(
        connection: &Connection,
        runtime: &tokio::runtime::Handle,
        jump: &Connection,
        known_hosts: std::path::PathBuf,
    ) -> Result<Self> {
        ensure!(connection.kind() == "RDP", "此接口仅用于 RDP SSH 路由");
        Self::start_configured(
            connection,
            runtime,
            Some(crate::terminal::ssh::tunnel::Route::via(
                connection,
                jump,
                known_hosts,
            )),
        )
    }
    fn start_configured(
        connection: &Connection,
        runtime: &tokio::runtime::Handle,
        ssh_route: Option<crate::terminal::ssh::tunnel::Route>,
    ) -> Result<Self> {
        crate::init_crypto();
        ensure!(
            matches!(connection.kind(), "RDP" | "VNC" | "ADB"),
            "此原生适配器仅处理 RDP/VNC/ADB"
        );
        let mut shared = Arc::new(Mutex::new(Shared::default()));
        let notifier: Notifier = Arc::new(Mutex::new(None));
        let clipboard = Arc::new(ClipboardBridge::default());
        let authentication = Arc::new(Authentication::default());
        let (input, inputs) = mpsc::channel(64);
        let mut fast_input = None;
        let mut adb_lease = None;
        let state = shared.clone();
        let repaint = notifier.clone();
        let task: Pin<Box<dyn Future<Output = ()>>> = if connection.kind() == "ADB" {
            let lease = adb::pool::acquire(connection, runtime, &notifier)?;
            shared = lease.shared();
            adb_lease = Some(lease);
            drop(inputs);
            Box::pin(std::future::pending())
        } else if connection.kind() == "VNC" {
            vnc::validate(connection)?;
            let connection = connection.clone();
            let bridge = clipboard.clone();
            let credentials = authentication.clone();
            // The worker guard aborts the native socket task when the owning tab
            // closes, including a partially read framebuffer or authentication.
            let mut worker = vnc::Worker(runtime.spawn(async move {
                let result =
                    vnc::run(connection, inputs, &state, &repaint, &bridge, &credentials).await;
                if let Err(error) = result {
                    if let Ok(mut state) = state.lock() {
                        state.status = Status::Failed(format!("{error:#}"));
                    }
                    wake(&repaint);
                }
                if let Ok(mut pending) = bridge.outgoing.lock() {
                    *pending = Default::default();
                }
            }));
            Box::pin(async move {
                let _ = (&mut worker.0).await;
                drop(worker);
            })
        } else {
            let mut config = configuration(connection)?;
            if let Some(route) = ssh_route.or_else(|| {
                connection
                    .flag("useSshTunnel")
                    .then(|| crate::terminal::ssh::tunnel::Route::new(connection))
            }) {
                config = config.with_tcp_connector(move |destination| {
                    let route = route.clone();
                    async move {
                        route
                            .connect(destination.name(), destination.port())
                            .await
                            .map_err(std::io::Error::other)
                    }
                });
            }
            let (task, sender) = worker::start(config, inputs, state, repaint, runtime.clone())?;
            fast_input = sender;
            task
        };
        let waker = Waker::from(Arc::new(RepaintWake(notifier.clone())));
        Ok(Self {
            adb: adb_lease,
            shared,
            input,
            fast_input,
            notifier,
            task,
            runtime: runtime.clone(),
            waker,
            finished: false,
            vnc: connection.kind() == "VNC",
            clipboard,
            authentication,
        })
    }
    /// Called from the application's logic phase for active and inactive tabs.
    /// Socket readiness wakes the event loop; this performs no blocking I/O.
    pub fn poll(&mut self) {
        if !self.finished {
            let _runtime = self.runtime.enter();
            self.finished = self
                .task
                .as_mut()
                .poll(&mut TaskContext::from_waker(&self.waker))
                .is_ready();
        }
    }
    pub fn status(&self) -> Status {
        self.shared
            .lock()
            .map(|s| s.status.clone())
            .unwrap_or(Status::Closed)
    }
    pub fn take_frame(&self) -> Option<Frame> {
        if let Some(adb) = &self.adb {
            return adb.frame();
        }
        let mut shared = self.shared.lock().ok()?;
        shared.frame.take().or_else(|| shared.rdp_surface.take())
    }
    /// Drain bounded native RGBA damage without a legacy RGB conversion.
    pub fn take_rdp_updates(&self) -> Vec<RgbaUpdate> {
        self.shared
            .lock()
            .map(|mut s| s.rdp_surface.take_rgba())
            .unwrap_or_default()
    }
    pub fn take_rdp_snapshots(&self) -> Vec<RdpSnapshot> {
        self.shared
            .lock()
            .map(|mut shared| shared.rdp_surface.take_snapshots())
            .unwrap_or_default()
    }
    pub fn set_yuv_output(&self, enabled: bool) {
        if let Some(adb) = &self.adb {
            adb.output(enabled);
            return;
        }
        if let Ok(mut shared) = self.shared.lock()
            && shared.adb_yuv_output != enabled
        {
            shared.adb_yuv_output = enabled;
            shared.adb_rgba_output = !enabled;
            shared.rgba_frame = None;
            shared.yuv_frame = None;
        }
    }
    pub fn take_yuv_frame(&self) -> Option<YuvFrame> {
        self.take_yuv_snapshot()
            .map(|frame| Arc::try_unwrap(frame).unwrap_or_else(|frame| (*frame).clone()))
    }
    /// Views and GPU callbacks share the same planar pixels without copying them.
    pub fn take_yuv_snapshot(&self) -> Option<Arc<YuvFrame>> {
        if let Some(adb) = &self.adb {
            return adb.yuv();
        }
        let mut shared = self.shared.lock().ok()?;
        let now = std::time::Instant::now();
        if shared
            .adb_frame_requested_at
            .is_some_and(|last| now.duration_since(last) > std::time::Duration::from_millis(250))
        {
            shared.yuv_frame = None;
        }
        shared.adb_frame_requested_at = Some(now);
        shared.yuv_frame.take().map(Arc::new)
    }
    pub fn take_rgba_frame(&self) -> Option<RgbaFrame> {
        if let Some(adb) = &self.adb {
            return adb.rgba();
        }
        let mut shared = self.shared.lock().ok()?;
        let now = std::time::Instant::now();
        if shared
            .adb_frame_requested_at
            .is_some_and(|last| now.duration_since(last) > std::time::Duration::from_millis(250))
        {
            shared.rgba_frame = None;
        }
        shared.adb_frame_requested_at = Some(now);
        shared.rgba_frame.take()
    }
    pub fn adb_video_stats(&self) -> Option<AdbVideoStats> {
        self.shared.lock().ok()?.adb_video
    }
    pub fn apply_adb_settings(&self, connection: &mut Connection) {
        if let Some(adb) = &self.adb {
            adb.settings(connection);
        }
    }
    pub fn shares_adb_transport(&self, other: &Self) -> bool {
        self.adb.is_some() && other.adb.is_some() && Arc::ptr_eq(&self.shared, &other.shared)
    }
    pub fn reconfigure_adb(&self, connection: &Connection) -> Result<()> {
        let adb = self.adb.as_ref().context("当前会话不是 ADB")?;
        adb.configure(connection);
        Ok(())
    }
    /// Rebuild a renderer from the retained canvas after a texture/viewport change.
    /// This shares the immutable canvas and does not consume pending network damage.
    pub fn rdp_full_snapshot(&self) -> Option<RdpSnapshot> {
        self.shared.lock().ok()?.rdp_surface.full_snapshot()
    }
    pub fn pointer_hidden(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|mut s| s.pointer_hidden || s.cursor_mode.hidden(std::time::Instant::now()))
    }
    pub fn pointer(&self) -> (u64, Option<Arc<Pointer>>) {
        self.shared
            .lock()
            .map(|s| (s.pointer_revision, s.pointer.clone()))
            .unwrap_or_default()
    }
    /// Queue-to-TLS completion only; excludes server processing and display latency.
    pub fn input_dispatch_stats(&self) -> Option<ironrdp_client::rdp::InputDispatchStats> {
        self.fast_input
            .as_ref()
            .map(ironrdp_client::rdp::FastPathSender::stats)
    }
    pub fn send(&self, event: Input) -> Result<()> {
        ensure!(self.status() == Status::Ready, "远程桌面尚未连接");
        if let Some(adb) = &self.adb {
            return adb.send(event);
        }
        if let Some(sender) = &self.fast_input
            && let RdpInputEvent::FastPath(events) = encode(event)
        {
            sender
                .try_send(events)
                .map_err(|_| anyhow::anyhow!("远程桌面输入队列已满或会话已关闭"))?;
            if let Input::Pointer {
                x,
                y,
                button,
                wheel: 0,
                ..
            } = event
            {
                let mut shared = self
                    .shared
                    .lock()
                    .map_err(|_| anyhow::anyhow!("RDP 状态锁失效"))?;
                let Shared {
                    rdp_surface,
                    cursor_mode,
                    ..
                } = &mut *shared;
                if button.is_some() {
                    cursor_mode.button(x, y);
                } else {
                    cursor_mode.motion(rdp_surface, x, y, std::time::Instant::now());
                }
            }
            return Ok(());
        }
        self.input
            .try_send(event)
            .map_err(|_| anyhow::anyhow!("远程桌面输入队列已满或会话已关闭"))
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut notifier) = self.notifier.lock() {
            *notifier = Some(Arc::new(callback));
        }
    }
    pub fn desktop_name(&self) -> String {
        self.shared
            .lock()
            .map(|s| s.desktop_name.clone())
            .unwrap_or_default()
    }
    pub fn take_clipboard(&self) -> Option<String> {
        self.shared.lock().ok()?.clipboard.take()
    }
    pub fn take_bell(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|mut s| std::mem::take(&mut s.bell))
    }
    pub fn send_clipboard(&self, text: String) -> Result<()> {
        self.queue_clipboard(text, false)
    }
    /// Reserve one clipboard transaction until its Ctrl+V key-up is delivered.
    pub fn prepare_paste(&self, text: String) -> Result<()> {
        self.queue_clipboard(text, true)
    }
    fn queue_clipboard(&self, text: String, paste: bool) -> Result<()> {
        ensure!(self.vnc, "RDP 剪贴板由原生协议组件同步");
        vnc::clipboard_length(&text)?;
        let mut state = self
            .clipboard
            .outgoing
            .lock()
            .map_err(|_| anyhow::anyhow!("剪贴板状态锁失效"))?;
        ensure!(self.status() == Status::Ready, "远程桌面尚未连接");
        ensure!(!state.busy, "上一次剪贴板传输尚未完成，请稍后重试");
        state.busy = true;
        state.pending = Some((text, paste));
        drop(state);
        self.clipboard.ready.notify_one();
        Ok(())
    }
    /// Resume the current RFB handshake without reconnecting the socket.
    pub fn provide_password(&self, password: String) -> Result<()> {
        ensure!(!password.is_empty(), "VNC 服务器需要密码");
        self.reply_to_password(PasswordReply::Provided(zeroize::Zeroizing::new(password)))
    }
    pub fn cancel_password(&self) -> Result<()> {
        self.reply_to_password(PasswordReply::Cancelled)
    }
    fn reply_to_password(&self, reply: PasswordReply) -> Result<()> {
        let mut state = self
            .shared
            .lock()
            .map_err(|_| anyhow::anyhow!("会话状态锁失效"))?;
        ensure!(
            state.status == Status::PasswordRequired,
            "当前会话未等待密码"
        );
        *self
            .authentication
            .reply
            .lock()
            .map_err(|_| anyhow::anyhow!("认证状态锁失效"))? = Some(reply);
        state.status = Status::Connecting;
        drop(state);
        self.authentication.ready.notify_one();
        wake(&self.notifier);
        Ok(())
    }
}

/// Old VNC profiles use the standard port when their port was unset or nonpositive.
pub fn vnc_port(connection: &Connection) -> u64 {
    match connection.number("port") {
        0 => 5900,
        port => port,
    }
}
pub use tomato_native_input::play_system_bell;
fn wake(notifier: &Notifier) {
    let callback = notifier.lock().ok().and_then(|value| value.clone());
    if let Some(callback) = callback {
        callback();
    }
}
fn configuration(connection: &Connection) -> Result<Config> {
    let host = connection.text("host");
    ensure!(!host.is_empty(), "RDP 主机不能为空");
    let port = u16::try_from(connection.number("port")).context("RDP 端口超出范围")?;
    ensure!(port > 0, "RDP 端口不能为空");
    let dimension = |key: &str, default| -> Result<u16> {
        let value = connection
            .options
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default);
        let value = u16::try_from(value)?;
        ensure!(
            (200..=8192).contains(&value),
            "RDP 分辨率必须在 200–8192 之间"
        );
        Ok(value)
    };
    let width = dimension("screenWidth", 1024)?;
    let height = dimension("screenHeight", 768)?;
    ensure!(
        usize::from(width) * usize::from(height) <= 16_777_216,
        "RDP 画面最多 1600 万像素"
    );
    let flag = |key, default| {
        connection
            .options
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    };
    let tls = flag("useSsl", true);
    let mut builder = ConfigBuilder::new()
        // Match Java Options' 0x6D policy: omit wallpaper, themes, cursor shadow
        // and cursor settings while keeping window contents visible on drag.
        // Also retain font smoothing requested for sharp remote text. Using
        // IronRDP's defaults here otherwise re-enables expensive backgrounds.
        .with_performance_flags(
            PerformanceFlags::DISABLE_WALLPAPER
                | PerformanceFlags::DISABLE_MENUANIMATIONS
                | PerformanceFlags::DISABLE_THEMING
                | PerformanceFlags::DISABLE_CURSOR_SHADOW
                | PerformanceFlags::DISABLE_CURSORSETTINGS
                | PerformanceFlags::ENABLE_FONT_SMOOTHING,
        )
        .with_graphics_pipeline(true)
        // Java Secure's earlyCaps omits network-characteristics autodetection.
        // Keep the original fixed session policy unless explicitly opted in.
        .with_network_autodetect(flag("rdpNetworkAutoDetect", false))
        .with_graphics_h264(flag("rdpAvc420", false))
        .with_sound(flag("enableSound", true))
        .with_destination(Destination::from_parts(host, port))
        .with_username(connection.text("username"))
        .with_password(connection.text("password"))
        .with_client_build(19045)
        .with_client_dir("C:\\Windows\\System32\\mstscax.dll")
        .with_client_name("Tomato")
        .with_platform(ironrdp_pdu::rdp::capability_sets::MajorPlatformType::WINDOWS)
        .with_desktop_width(width)
        .with_desktop_height(height)
        // The original 24-bit setting maps to the engine's 32-bit RGB framebuffer.
        .with_color_depth(if connection.number("colorDepth") == 16 {
            16
        } else {
            32
        })
        .with_tls(tls)
        .with_credssp(tls)
        .with_autologon(true)
        .with_server_pointer(true)
        .with_pointer_software_rendering(false)
        .with_clipboard(if flag("mapClipboard", true) {
            ClipboardType::Enable
        } else {
            ClipboardType::Disable
        });
    if !connection.text("domain").is_empty() {
        builder = builder.with_domain(connection.text("domain"));
    }
    // Java retries a server's explicit SSL_NOT_ALLOWED/SSL_REQUIRED response.
    // Preserve that behavior without changing the saved useSsl setting.
    builder
        .build()
        .map(|config| config.with_legacy_security_fallback(true))
}
fn encode(event: Input) -> RdpInputEvent {
    let event = match event {
        Input::AndroidKey(_) => return RdpInputEvent::FastPath(Default::default()),
        Input::Resize { width, height } => {
            return RdpInputEvent::Resize {
                width,
                height,
                scale_factor: 100,
                physical_size: None,
            };
        }
        Input::ScanCode {
            code,
            extended,
            down,
        } => {
            let mut flags = KeyboardFlags::empty();
            if extended {
                flags |= KeyboardFlags::EXTENDED;
            }
            if !down {
                flags |= KeyboardFlags::RELEASE;
            }
            FastPathInputEvent::KeyboardEvent(flags, code)
        }
        Input::Unicode { value, down } => FastPathInputEvent::UnicodeKeyboardEvent(
            if down {
                KeyboardFlags::empty()
            } else {
                KeyboardFlags::RELEASE
            },
            value,
        ),
        Input::Pointer {
            x,
            y,
            button,
            down,
            wheel,
        } => {
            let mut flags = match button {
                Some(1) => PointerFlags::LEFT_BUTTON,
                Some(2) => PointerFlags::RIGHT_BUTTON,
                Some(3) => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
                _ => PointerFlags::MOVE,
            };
            if button.is_some() && down {
                flags |= PointerFlags::DOWN;
            }
            if wheel != 0 {
                flags = PointerFlags::VERTICAL_WHEEL;
            }
            FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units: wheel.clamp(-256, 255),
                x_position: x,
                y_position: y,
            })
        }
    };
    RdpInputEvent::FastPath(std::iter::once(event).collect())
}
async fn drive_rdp(
    engine: impl Future<Output = ()>,
    sender: ironrdp_client::rdp::InputSender,
    mut input: mpsc::Receiver<Input>,
    mut output: mpsc::Receiver<RdpOutputEvent>,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    tokio::pin!(engine);
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(30));
    tokio::pin!(timeout);
    let mut ready = false;
    let mut engine_done = false;
    loop {
        // Forward input before more decoding, then advance the producer before
        // receiving its output. The bounded retained canvas coalesces GUI lag.
        tokio::select! {
            biased;
            _ = &mut timeout, if !ready => anyhow::bail!("RDP 连接超时"),
            event = input.recv() => match event { Some(event) => {
                if let Input::Pointer {x,y,button,wheel:0,..}=event {
                    let mut state=shared.lock().map_err(|_|anyhow::anyhow!("RDP 状态锁失效"))?;
                    let Shared {rdp_surface,cursor_mode,..}=&mut *state;
                    if button.is_some() {cursor_mode.button(x,y);} else {cursor_mode.motion(rdp_surface,x,y,std::time::Instant::now());}
                }
                sender.send(encode(event)).map_err(|_| anyhow::anyhow!("RDP 会话已关闭"))?;
            }, None => break },
            _ = &mut engine, if !engine_done => engine_done = true,
            event = output.recv() => {
                let Some(event) = event else { break; };
                let frame_ready=matches!(&event,RdpOutputEvent::Image{..}|RdpOutputEvent::ImageRegion{..}|RdpOutputEvent::RgbaRegions{..});
                {
                let mut state = shared.lock().map_err(|_| anyhow::anyhow!("RDP 状态锁失效"))?;
                match event {
                    RdpOutputEvent::Image {buffer,width,height} => {
                        ensure!(buffer.len() == usize::from(width.get()) * usize::from(height.get()) && buffer.len() <= 16_777_216, "RDP 帧大小无效");
                        state.rdp_surface.apply(width.get(),height.get(),Region{x:0,y:0,width:width.get(),height:height.get()},buffer)?;
                        state.status = Status::Ready;
                        ready = true;
                    },
                    RdpOutputEvent::ImageRegion {buffer,width,height,region} => {
                        ensure!(region.left<=region.right && region.top<=region.bottom,"RDP 更新范围无效");
                        state.rdp_surface.apply(width.get(),height.get(),Region{x:region.left,y:region.top,width:region.right-region.left+1,height:region.bottom-region.top+1},buffer)?;
                        state.status=Status::Ready;
                        ready=true;
                    },
                    RdpOutputEvent::RgbaRegions {width,height,regions} => {
                        ensure!(regions.len() <= 32, "RDP 更新数量无效");
                        for (region, buffer) in regions {
                            ensure!(region.left<=region.right && region.top<=region.bottom,"RDP 更新范围无效");
                            state.rdp_surface.apply_rgba(width.get(),height.get(),Region{x:region.left,y:region.top,width:region.right-region.left+1,height:region.bottom-region.top+1},buffer)?;
                        }
                        state.status=Status::Ready;
                        ready=true;
                    },
                    RdpOutputEvent::ConnectionFailure(error) => return Err(anyhow::Error::new(error).context("RDP 连接失败")),
                    RdpOutputEvent::Terminated(reason) => {
                        reason.context("RDP 会话异常结束")?;
                        state.status = Status::Closed;
                        drop(state); wake(notifier); return Ok(());
                    },
                    RdpOutputEvent::PointerHidden => {state.pointer_hidden = true;state.cursor_mode.shape(None,true);},
                    RdpOutputEvent::PointerDefault => {
                        state.cursor_mode.shape(None,false);
                        state.pointer_hidden = false;
                        state.pointer = None;
                        state.pointer_revision = state.pointer_revision.wrapping_add(1);
                    },
                    RdpOutputEvent::PointerBitmap(pointer) => {
                        ensure!(pointer.width > 0 && pointer.height > 0 && pointer.width <= 384 && pointer.height <= 384
                            && pointer.bitmap_data.len() == usize::from(pointer.width)*usize::from(pointer.height)*4, "RDP 光标大小无效");
                        state.pointer_hidden = false;
                        state.pointer = Some(Arc::new(Pointer {size:[pointer.width,pointer.height],hotspot:[pointer.hotspot_x,pointer.hotspot_y],rgba:pointer.bitmap_data.clone()}));
                        let transparent=pointer.bitmap_data.as_chunks::<4>().0.iter().all(|p|p[3]==0);
                        let retained=state.pointer.clone();
                        state.cursor_mode.shape(retained.as_deref(),transparent);
                        state.pointer_hidden=transparent;
                        state.pointer_revision = state.pointer_revision.wrapping_add(1);
                    },
                    RdpOutputEvent::PointerPosition {..} => state.cursor_mode.server_position(),
                }
                if frame_ready {
                    let Shared {rdp_surface,cursor_mode,..}=&mut *state;
                    cursor_mode.frame(rdp_surface,std::time::Instant::now());
                }
                }
                wake(notifier);
                // Publish before yielding. A producer with more ready pictures
                // must not keep this GUI poll busy draining its entire stream.
                if frame_ready { tokio::task::yield_now().await; }
            }
        }
    }
    if let Ok(mut state) = shared.lock() {
        state.status = Status::Closed;
    }
    wake(notifier);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn decoded_picture_is_visible_in_the_same_poll_and_ready_stream_yields_each_batch() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let notifier: Notifier = Arc::new(Mutex::new(None));
        let (_input_owner, input) = mpsc::channel(1);
        let (wire, _wire_owner) = mpsc::unbounded_channel::<RdpInputEvent>();
        let (pictures, output) = mpsc::channel(1);
        let producer = async move {
            for version in 0..64u8 {
                pictures
                    .send(RdpOutputEvent::RgbaRegions {
                        width: std::num::NonZeroU16::new(1).unwrap(),
                        height: std::num::NonZeroU16::new(1).unwrap(),
                        regions: vec![(
                            ironrdp_pdu::geometry::InclusiveRectangle {
                                left: 0,
                                top: 0,
                                right: 0,
                                bottom: 0,
                            },
                            vec![u32::from_ne_bytes([version, 34, 56, 255])],
                        )],
                    })
                    .await
                    .unwrap();
            }
            std::future::pending::<()>().await;
            drop(pictures);
        };
        // GUI-owned futures are manually polled outside a Tokio task. Disable
        // the test task's implicit budget so only the actor's own fairness counts.
        let mut actor = Box::pin(tokio::task::unconstrained(drive_rdp(
            producer,
            wire.into(),
            input,
            output,
            &shared,
            &notifier,
        )));
        let mut context = TaskContext::from_waker(Waker::noop());
        for version in 0..64u8 {
            // Simulate exactly one GUI poll. No timers/sleeps or subsequent poll
            // may be needed to expose the newly decoded picture to the renderer.
            assert!(actor.as_mut().poll(&mut context).is_pending());
            let mut state = shared.lock().unwrap();
            let updates = state.rdp_surface.take_rgba();
            assert_eq!(
                updates.len(),
                1,
                "picture delayed or ready stream not bounded to one batch"
            );
            assert_eq!(
                updates[0].frame.pixels,
                vec![u32::from_ne_bytes([version, 34, 56, 255])]
            );
            assert_eq!(state.status, Status::Ready);
        }
    }
    #[test]
    fn legacy_rdp_options_map_without_changing_the_saved_connection() {
        let mut c = Connection::new("RDP");
        c.options.extend(serde_json::json!({"host":"127.0.0.1","username":"fixture","password":"fixture-only","domain":"EXAMPLE","screenWidth":1280,"screenHeight":720,"colorDepth":24,"useSsl":true,"mapClipboard":false}).as_object().unwrap().clone());
        let original = c.clone();
        let config = configuration(&c).unwrap();
        assert_eq!(config.destination().port(), 3389);
        assert_eq!(config.connector().desktop_size.width, 1280);
        assert_eq!(config.connector().desktop_size.height, 720);
        assert_eq!(config.connector().domain.as_deref(), Some("EXAMPLE"));
        assert!(config.connector().enable_tls && config.connector().enable_credssp);
        assert_eq!(config.connector().bitmap.as_ref().unwrap().color_depth, 32);
        // Original Java Options: 0x6D. Preserve those settings plus 0x80 font
        // smoothing, without reintroducing the outline-only drag bit (0x02).
        assert_eq!(config.connector().performance_flags.bits(), 0xED);
        assert!(!config.connector().enable_network_autodetect);
        assert!(
            !config
                .connector()
                .performance_flags
                .contains(PerformanceFlags::DISABLE_FULLWINDOWDRAG)
        );
        assert!(
            config
                .connector()
                .performance_flags
                .contains(PerformanceFlags::ENABLE_FONT_SMOOTHING)
        );
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        c.options.insert("screenWidth".into(), 100_000.into());
        assert!(configuration(&c).is_err());
    }
    #[test]
    fn input_retains_extended_keys_release_and_signed_wheel() {
        let RdpInputEvent::FastPath(events) = encode(Input::ScanCode {
            code: 0x1d,
            extended: true,
            down: false,
        }) else {
            panic!()
        };
        assert_eq!(
            events[0],
            FastPathInputEvent::KeyboardEvent(
                KeyboardFlags::EXTENDED | KeyboardFlags::RELEASE,
                0x1d
            )
        );
        let RdpInputEvent::FastPath(events) = encode(Input::Pointer {
            x: 32,
            y: 42,
            button: None,
            down: false,
            wheel: -120,
        }) else {
            panic!()
        };
        let FastPathInputEvent::MouseEvent(mouse) = events[0] else {
            panic!()
        };
        assert_eq!(mouse.number_of_wheel_rotation_units, -120);
        assert_eq!((mouse.x_position, mouse.y_position), (32, 42));
    }
}
