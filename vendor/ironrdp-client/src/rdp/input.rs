//! A bounded input lane and one TLS writer, independent of synchronous image decoding.
use super::*;
use ironrdp_pdu::input::fast_path::FastPathInput;
use std::io;
use std::sync::{Mutex, atomic::AtomicBool};
use tokio::sync::oneshot;

type Events = SmallVec<[FastPathInputEvent; 2]>;
struct Queued {
    events: Events,
    at: std::time::Instant,
    after_control: u64,
}

pub(super) struct Control {
    pub(super) event: RdpInputEvent,
    pub(super) sequence: u64,
}

#[derive(Clone)]
enum Destination {
    Ordered(mpsc::UnboundedSender<Control>),
    Plain(mpsc::UnboundedSender<RdpInputEvent>),
}

/// All production control producers share the fast lane's enqueue sequencer.
#[derive(Clone)]
pub struct InputSender {
    destination: Destination,
    state: Option<Arc<State>>,
}
impl std::fmt::Debug for InputSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputSender").finish_non_exhaustive()
    }
}
impl From<mpsc::UnboundedSender<RdpInputEvent>> for InputSender {
    /// Compatibility for standalone protocol mocks, without a fast lane.
    fn from(sender: mpsc::UnboundedSender<RdpInputEvent>) -> Self {
        Self {
            destination: Destination::Plain(sender),
            state: None,
        }
    }
}
impl InputSender {
    pub(super) fn channel(lane: Option<&Lane>) -> (Self, mpsc::UnboundedReceiver<Control>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            Self {
                destination: Destination::Ordered(sender),
                state: lane.map(|lane| lane.sender.state.clone()),
            },
            receiver,
        )
    }
    pub fn send(&self, event: RdpInputEvent) -> Result<(), mpsc::error::SendError<RdpInputEvent>> {
        match &self.destination {
            Destination::Plain(sender) => sender.send(event),
            Destination::Ordered(sender) => {
                // No await or protocol work while holding this lock. Publishing
                // the sequence and enqueueing the command must be atomic with
                // respect to a later GUI key/button/motion enqueue.
                let mut issued = self.state.as_ref().map(|state| {
                    state
                        .issued_control
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                });
                let sequence = issued.as_ref().map_or(0, |issued| issued.saturating_add(1));
                sender
                    .send(Control { event, sequence })
                    .map_err(|error| mpsc::error::SendError(error.0.event))?;
                if let Some(issued) = issued.as_mut() {
                    **issued = sequence;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputDispatchStats {
    pub messages: u64,
    pub mean_micros: u64,
    pub max_micros: u64,
}

#[derive(Clone)]
pub struct FastPathSender {
    sender: mpsc::Sender<Queued>,
    state: Arc<State>,
}
struct State {
    active: AtomicBool,
    paused: AtomicBool,
    changed: tokio::sync::Notify,
    sent: AtomicU64,
    total_nanos: AtomicU64,
    max_nanos: AtomicU64,
    activity: Mutex<(tokio::time::Instant, Option<(u16, u16)>)>,
    issued_control: Mutex<u64>,
    completed_control: AtomicU64,
}
impl FastPathSender {
    pub fn try_send(&self, events: Events) -> Result<(), mpsc::error::TrySendError<Events>> {
        if !self.state.active.load(Ordering::Acquire) {
            return Err(mpsc::error::TrySendError::Closed(events));
        }
        let mouse = events.iter().rev().find_map(|event| match event {
            FastPathInputEvent::MouseEvent(mouse) => Some((mouse.x_position, mouse.y_position)),
            FastPathInputEvent::MouseEventEx(mouse) => Some((mouse.x_position, mouse.y_position)),
            _ => None,
        });
        let issued = self
            .state
            .issued_control
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.sender
            .try_send(Queued {
                events,
                at: std::time::Instant::now(),
                after_control: *issued,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(queued) => {
                    mpsc::error::TrySendError::Full(queued.events)
                }
                mpsc::error::TrySendError::Closed(queued) => {
                    mpsc::error::TrySendError::Closed(queued.events)
                }
            })?;
        drop(issued);
        if let Ok(mut activity) = self.state.activity.lock() {
            activity.0 = tokio::time::Instant::now();
            if mouse.is_some() {
                activity.1 = mouse;
            }
        }
        Ok(())
    }
    pub fn stats(&self) -> InputDispatchStats {
        let messages = self.state.sent.load(Ordering::Relaxed);
        InputDispatchStats {
            messages,
            mean_micros: self
                .state
                .total_nanos
                .load(Ordering::Relaxed)
                .checked_div(messages)
                .unwrap_or(0)
                / 1000,
            max_micros: self.state.max_nanos.load(Ordering::Relaxed) / 1000,
        }
    }
}

#[derive(Clone)]
pub(super) struct Lane {
    sender: FastPathSender,
    receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<Queued>>>,
}
impl Lane {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel(64);
        Self {
            sender: FastPathSender {
                sender,
                state: Arc::new(State {
                    active: AtomicBool::new(false),
                    paused: AtomicBool::new(false),
                    changed: tokio::sync::Notify::new(),
                    sent: AtomicU64::new(0),
                    total_nanos: AtomicU64::new(0),
                    max_nanos: AtomicU64::new(0),
                    activity: Mutex::new((tokio::time::Instant::now(), None)),
                    issued_control: Mutex::new(0),
                    completed_control: AtomicU64::new(0),
                }),
            },
            receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
        }
    }
    pub(super) fn sender(&self) -> FastPathSender {
        self.sender.clone()
    }
    pub(super) fn activity(&self) -> Option<(tokio::time::Instant, Option<(u16, u16)>)> {
        self.sender
            .state
            .activity
            .lock()
            .ok()
            .map(|activity| *activity)
    }
    pub(super) fn pause(&self, paused: bool) {
        self.sender.state.paused.store(paused, Ordering::Release);
        self.sender.state.changed.notify_one();
    }
    pub(super) fn complete_control(&self, sequence: u64) {
        self.sender
            .state
            .completed_control
            .fetch_max(sequence, Ordering::Release);
        self.sender.state.changed.notify_one();
    }
}

struct Request {
    bytes: Vec<u8>,
    complete: oneshot::Sender<io::Result<()>>,
}
pub(super) struct Writer {
    sender: mpsc::Sender<Request>,
    task: tokio::task::JoinHandle<()>,
    state: Option<Arc<State>>,
}
struct Active(Arc<State>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::Release);
    }
}
impl Writer {
    pub(super) fn start<W>(mut writer: W, lane: Option<Lane>) -> Self
    where
        W: FramedWrite + Send + 'static,
        for<'a> W::WriteAllFut<'a>: Send,
    {
        let (sender, mut requests) = mpsc::channel::<Request>(1);
        let state = lane.as_ref().map(|lane| lane.sender.state.clone());
        let task = tokio::spawn(async move {
            let mut input = match lane.as_ref() {
                Some(lane) => Some(lane.receiver.lock().await),
                None => None,
            };
            // Never replay a previous activation's queued keys after reconnecting.
            if let Some(input) = input.as_mut() {
                while input.try_recv().is_ok() {}
            }
            let _active = lane.as_ref().map(|lane| {
                lane.pause(false);
                lane.sender.state.active.store(true, Ordering::Release);
                Active(lane.sender.state.clone())
            });
            let mut pending: Option<Queued> = None;
            loop {
                if lane.as_ref().is_none_or(|lane| {
                    !lane.sender.state.paused.load(Ordering::Acquire)
                        && pending.as_ref().is_none_or(|queued| {
                            lane.sender.state.completed_control.load(Ordering::Acquire)
                                >= queued.after_control
                        })
                }) && let Some(queued) = pending.take()
                {
                    for chunk in queued.events.chunks(FastPathInput::MAX_EVENTS) {
                        let Ok(pdu) = FastPathInput::new(chunk.to_vec()) else {
                            return;
                        };
                        let Ok(bytes) = ironrdp_core::encode_vec(&pdu) else {
                            return;
                        };
                        if writer.write_all(&bytes).await.is_err() {
                            return;
                        }
                    }
                    if let Some(lane) = &lane {
                        let nanos =
                            u64::try_from(queued.at.elapsed().as_nanos()).unwrap_or(u64::MAX);
                        let state = &lane.sender.state;
                        state.sent.fetch_add(1, Ordering::Relaxed);
                        state.total_nanos.fetch_add(nanos, Ordering::Relaxed);
                        state.max_nanos.fetch_max(nanos, Ordering::Relaxed);
                    }
                    continue;
                }
                tokio::select! {
                    events = async { match input.as_mut() {
                        Some(input) => input.recv().await,
                        None => std::future::pending().await,
                    }}, if pending.is_none() => {
                        let Some(events) = events else { break; };
                        pending = Some(events);
                    }
                    request = requests.recv() => {
                        let Some(request) = request else { break; };
                        let result = writer.write_all(&request.bytes).await;
                        let failed = result.is_err();
                        let _ = request.complete.send(result);
                        if failed { break; }
                    }
                    _ = async { match lane.as_ref() {
                        Some(lane) => lane.sender.state.changed.notified().await,
                        None => std::future::pending().await,
                    }} => {}
                }
            }
        });
        Self {
            sender,
            task,
            state,
        }
    }
    pub(super) async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_owned(bytes.to_vec()).await
    }
    pub(super) async fn write_owned(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let (complete, finished) = oneshot::channel();
        self.sender
            .send(Request { bytes, complete })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "RDP writer stopped"))?;
        finished
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "RDP write interrupted"))?
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            state.active.store(false, Ordering::Release);
        }
        self.task.abort();
    }
}

#[cfg(test)]
mod tests;
