//! An owned STA drag loop keeps the application responsive during delayed rendering.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering},
    mpsc,
};
use windows::Win32::{
    Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, LPARAM, WPARAM,
    },
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyboardState, ReleaseCapture, SetKeyboardState, VK_ESCAPE,
        },
        WindowsAndMessaging::*,
    },
};
mod asynchronous;
#[cfg(test)]
mod batch_tests;
mod data;
mod descriptor;
mod lazy_stream;
#[cfg(test)]
mod manifest_tests;
mod source;
mod stream;
const PENDING: HRESULT = HRESULT(0x8000000A_u32 as i32);
type Lease = (std::path::PathBuf, Arc<dyn Any + Send + Sync>);
type Resolve = Box<dyn FnOnce(Cancellation) -> Result<Lease> + Send>;
type Ready = Box<dyn Fn() -> Option<Lease> + Send>;
type Available = Box<dyn Fn(u64, u32, Cancellation) -> Result<u32> + Send>;
type Expand = Box<dyn FnOnce(Cancellation) -> Result<Vec<File>> + Send>;
pub struct File {
    name: String,
    size: Option<u64>,
    directory: bool,
    ready: Ready,
    resolve: Resolve,
    available: Option<Available>,
}
impl File {
    pub fn new(
        name: String,
        size: Option<u64>,
        ready: impl Fn() -> Option<Lease> + Send + 'static,
        resolve: impl FnOnce(Cancellation) -> Result<Lease> + Send + 'static,
    ) -> Self {
        Self {
            name,
            size,
            directory: false,
            ready: Box::new(ready),
            resolve: Box::new(resolve),
            available: None,
        }
    }
    pub fn with_directory(mut self, directory: bool) -> Self {
        self.directory = directory;
        self
    }
    /// Wait for the next committed prefix, rather than the whole source file.
    pub fn with_available(
        mut self,
        available: impl Fn(u64, u32, Cancellation) -> Result<u32> + Send + 'static,
    ) -> Self {
        self.available = Some(Box::new(available));
        self
    }
}
struct InputQueue {
    worker: u32,
    source: u32,
}
impl InputQueue {
    fn attach(source: u32) -> Result<Self> {
        let worker = unsafe { GetCurrentThreadId() };
        unsafe {
            AttachThreadInput(worker, source, true).ok()?;
        }
        Ok(Self { worker, source })
    }
}
impl Drop for InputQueue {
    fn drop(&mut self) {
        unsafe {
            let _ = AttachThreadInput(self.worker, self.source, false);
        }
    }
}
#[derive(Debug, Default)]
struct Progress {
    queries: AtomicU32,
    feedback: AtomicU32,
    keys: AtomicU32,
    effect: AtomicU32,
    result: AtomicI32,
    finished: AtomicBool,
    asynchronous: AtomicBool,
}
#[derive(Debug)]
pub struct Diagnostics {
    pub queries: u32,
    pub feedback: u32,
    pub keys: u32,
    pub effect: u32,
    pub result: Option<i32>,
    pub released: bool,
    pub asynchronous: bool,
}
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>, Arc<AtomicBool>);
impl Cancellation {
    fn new() -> Self {
        Self(
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        )
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) || unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) < 0 }
    }
}
pub struct Drag {
    cancel: Cancellation,
    thread: Arc<AtomicU32>,
    replies: mpsc::Receiver<std::result::Result<bool, String>>,
    _worker: std::thread::JoinHandle<()>,
    progress: Arc<Progress>,
}
impl Drag {
    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            queries: self.progress.queries.load(Ordering::Relaxed),
            feedback: self.progress.feedback.load(Ordering::Relaxed),
            keys: self.progress.keys.load(Ordering::Relaxed),
            effect: self.progress.effect.load(Ordering::Relaxed),
            result: self
                .progress
                .finished
                .load(Ordering::Acquire)
                .then(|| self.progress.result.load(Ordering::Relaxed)),
            released: self.cancel.1.load(Ordering::Acquire),
            asynchronous: self.progress.asynchronous.load(Ordering::Acquire),
        }
    }
    pub fn try_result(&self) -> Option<Result<bool>> {
        match self.replies.try_recv() {
            Ok(result) => Some(result.map_err(anyhow::Error::msg)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("系统拖放线程已关闭")))
            }
        }
    }
}
impl Drop for Drag {
    fn drop(&mut self) {
        self.cancel.0.store(true, Ordering::Release);
        let id = self.thread.load(Ordering::Acquire);
        if id != 0 {
            // Only this job's STA is interrupted, never the application message loop.
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}
pub fn begin(
    name: &str,
    ready: impl Fn() -> Option<Lease> + Send + 'static,
    resolve: impl FnOnce(Cancellation) -> Result<Lease> + Send + 'static,
) -> Result<Drag> {
    begin_files(vec![File::new(name.to_owned(), None, ready, resolve)])
}
pub fn begin_files(files: Vec<File>) -> Result<Drag> {
    begin_catalog(files, None)
}
/// Publish the selected roots immediately; expand directory metadata only on drop.
pub fn begin_deferred_files(
    preview: Vec<File>,
    expand: impl FnOnce(Cancellation) -> Result<Vec<File>> + Send + 'static,
) -> Result<Drag> {
    begin_catalog(preview, Some(Box::new(expand)))
}
fn begin_catalog(files: Vec<File>, expand: Option<Expand>) -> Result<Drag> {
    let descriptor = descriptor::Descriptor::new(&files)?;
    let source_thread = unsafe { GetCurrentThreadId() };
    let mut keys = [0; 256];
    unsafe {
        GetKeyboardState(&mut keys)?;
    }
    // Winit owns capture after the initial press; OLE must own it for the drag loop.
    unsafe {
        let _ = ReleaseCapture();
    }
    let cancel = Cancellation::new();
    let thread = Arc::new(AtomicU32::new(0));
    let (send, replies) = mpsc::sync_channel(1);
    let (control, identity) = (cancel.clone(), thread.clone());
    let progress = Arc::new(Progress::default());
    let worker_progress = progress.clone();
    let worker = std::thread::Builder::new()
        .name("tomato-file-drag".into())
        .spawn(move || {
            let result = (|| {
                let _apartment = Apartment::new()?;
                let mut message = MSG::default();
                unsafe {
                    let _ = PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE);
                }
                // OLE's capture window must share the foreground source's input queue.
                // The lease detaches on success, cancellation, and initialization failure.
                let input = InputQueue::attach(source_thread)?;
                unsafe {
                    // A new STA has not processed the source window's button-down.
                    // Seed its queue state before OLE takes capture; subsequent input
                    // belongs to OLE. This never changes the physical input state.
                    SetKeyboardState(&keys)?;
                }
                identity.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
                if control.cancelled() {
                    return Ok(false);
                }
                let deferred =
                    data::Deferred::with_expansion(descriptor, files, control.clone(), expand);
                let operation = deferred.operation.clone();
                let object: IDataObject = deferred.into();
                {
                    use windows::core::Interface as _;
                    let asynchronous: IDataObjectAsyncCapability = object.cast()?;
                    unsafe {
                        asynchronous.SetAsyncMode(true)?;
                    }
                }
                let source: IDropSource =
                    source::Source::new(control.clone(), worker_progress.clone()).into();
                let mut effect = DROPEFFECT_NONE;
                // SHDoDragDrop adds a Shell drag-image data writer and default source.
                // Our read-only delayed data uses the OLE source contract directly.
                let result = unsafe { DoDragDrop(&object, &source, DROPEFFECT_COPY, &mut effect) };
                // Explorer may keep extracting on its copy worker. Its UI and the
                // application's queue must be independent while COM calls are pumped.
                drop(input);
                if result == DRAGDROP_S_DROP && operation.negotiated() {
                    worker_progress.asynchronous.store(true, Ordering::Release);
                    let completed = operation.wait(&control);
                    worker_progress.effect.store(effect.0, Ordering::Relaxed);
                    worker_progress.result.store(
                        completed
                            .as_ref()
                            .map_or(0x80004005_u32 as i32, |_| result.0),
                        Ordering::Relaxed,
                    );
                    worker_progress.finished.store(true, Ordering::Release);
                    return completed;
                }
                worker_progress.effect.store(effect.0, Ordering::Relaxed);
                worker_progress.result.store(result.0, Ordering::Relaxed);
                worker_progress.finished.store(true, Ordering::Release);
                result.ok()?;
                Ok(result == DRAGDROP_S_DROP && effect.0 & DROPEFFECT_COPY.0 != 0)
            })()
            .map_err(|error: anyhow::Error| format!("{error:#}"));
            identity.store(0, Ordering::Release);
            let _ = send.send(result);
        })?;
    Ok(Drag {
        cancel,
        thread,
        replies,
        _worker: worker,
        progress,
    })
}
