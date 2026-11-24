//! Cancellable RDP protocol worker with a dedicated native clipboard STA.
use super::*;
use tokio::sync::oneshot;
type Completion = Pin<Box<dyn Future<Output = ()>>>;
type Started = (Completion, Option<ironrdp_client::rdp::FastPathSender>);
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FINISHED: tokio::sync::Notify = tokio::sync::Notify::const_new();
struct Complete;
impl Complete {
    fn new() -> Self {
        ACTIVE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self
    }
}
impl Drop for Complete {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        FINISHED.notify_waiters();
    }
}
pub(super) async fn wait_cleanup() {
    loop {
        let notified = FINISHED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE.load(std::sync::atomic::Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}

struct Cancel {
    signal: Option<oneshot::Sender<()>>,
    clipboard: Box<dyn Fn() + Send + Sync>,
}
impl Drop for Cancel {
    fn drop(&mut self) {
        (self.clipboard)();
        if let Some(cancel) = self.signal.take() {
            let _ = cancel.send(());
        }
    }
}

pub(super) fn start(
    config: Config,
    input: mpsc::Receiver<Input>,
    state: Arc<Mutex<Shared>>,
    notifier: Notifier,
    runtime: tokio::runtime::Handle,
) -> Result<Started> {
    let (output_sender, output) = mpsc::channel(1);
    let client = RdpClient::new(config, output_sender);
    let sender = client.input_sender();
    let fast_input = client.fastpath_sender();
    // The native clipboard has its own STA/message pump. Its cancellation handle
    // is Send; protocol and clipboard teardown never wait on the application UI.
    let (client, clipboard) = client
        .prepare()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let clipboard_stop = Box::new(clipboard.cancellation());
    let (stop, stopped) = oneshot::channel();
    let (done, finished) = oneshot::channel();
    let complete = Complete::new();
    std::thread::Builder::new()
        .name("tomato-rdp".into())
        .spawn(move || {
            let _complete = complete;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = stopped => Ok(()),
                        result = drive_rdp(client.run(), sender, input, output, &state, &notifier) => result,
                    }
                })
            }));
            let error = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("{error:#}")),
                Err(_) => Some("RDP 后台任务异常结束".into()),
            };
            if let Some(error) = error {
                if let Ok(mut shared) = state.lock() {
                    shared.status = Status::Failed(error);
                }
                wake(&notifier);
            }
            clipboard.shutdown();
            let _ = done.send(());
        })
        .context("无法启动 RDP 解码任务")?;
    Ok((
        owned_completion(
            Cancel {
                signal: Some(stop),
                clipboard: clipboard_stop,
            },
            finished,
        ),
        fast_input,
    ))
}

fn owned_completion(
    cancel: Cancel,
    mut finished: oneshot::Receiver<()>,
) -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(async move {
        let _cancel = cancel;
        let _ = (&mut finished).await;
    })
}
