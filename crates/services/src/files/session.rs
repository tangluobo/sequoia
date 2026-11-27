//! Bounded actor for file backends, sharing the SSH workspace request contract.
use super::{
    operations::{Operation, Output, Reply, Request},
    storage::Storage,
};
use crate::terminal::Status;
use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tomato_core::Connection;
type Notify = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
pub struct FileSession {
    requests: mpsc::Sender<Request>,
    replies: mpsc::Receiver<Reply>,
    state: Arc<Mutex<Status>>,
    notify: Notify,
    task: tokio::task::JoinHandle<()>,
    next: u64,
    stopped: bool,
}
fn wake(notify: &Notify) {
    let callback = notify.lock().ok().and_then(|callback| callback.clone());
    if let Some(callback) = callback {
        callback();
    }
}
impl FileSession {
    pub fn start(runtime: &tokio::runtime::Handle, connection: Connection) -> Self {
        Self::start_with_context(runtime, connection, super::object_copy::Context::default())
    }
    pub fn start_with_context(
        runtime: &tokio::runtime::Handle,
        connection: Connection,
        context: super::object_copy::Context,
    ) -> Self {
        let (requests, mut receive) = mpsc::channel::<Request>(8);
        let (send, replies) = mpsc::channel(8);
        let state = Arc::new(Mutex::new(Status::Connecting));
        let notify: Notify = Default::default();
        let task_state = state.clone();
        let task_notify = notify.clone();
        let task = runtime.spawn(async move {
            let result = async {
                let storage = Storage::connect(&connection).await?.with_copy_context(context);
                storage.check().await?;
                *task_state.lock().map_err(|_|anyhow::anyhow!("文件状态锁失效"))? = Status::Ready;
                wake(&task_notify);
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
                timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut editing = false;
                loop {
                    tokio::select! {
                        request = receive.recv() => {
                            let Some(request) = request else { break; };
                            let result = storage.execute(request.operation).await;
                            editing |= matches!(&result, Ok(Output::EditCached { .. }));
                            let result = result.map_err(|e|format!("{e:#}"));
                            if let Some(response) = request.response { let _ = response.send(result); }
                            else { send.send(Reply { id: request.id, result }).await?; }
                            wake(&task_notify);
                        }
                        _ = timer.tick(), if editing => {
                            let result = storage.editors.lock().await.synchronize(&storage).await;
                            let output = match result { Ok(Some(text)) => Some(Ok(Output::Done(text))), Ok(None) => None, Err(error) => Some(Err(format!("编辑回传失败：{error:#}"))) };
                            if let Some(result) = output { send.send(Reply { id: 0, result }).await?; wake(&task_notify); }
                        }
                    }
                }
                Ok::<_,anyhow::Error>(())
            }.await;
            if let Ok(mut state) = task_state.lock() {
                *state = match result { Ok(()) => Status::Exited, Err(e) => Status::Failed(format!("{e:#}")) };
            }
            wake(&task_notify);
        });
        Self {
            requests,
            replies,
            state,
            notify,
            task,
            next: 0,
            stopped: false,
        }
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut notify) = self.notify.lock() {
            *notify = Some(Arc::new(callback));
        }
        wake(&self.notify);
    }
    pub fn status(&self) -> Status {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| Status::Failed("文件状态锁失效".into()))
    }
    pub fn request(&mut self, operation: Operation) -> Result<u64> {
        self.next = self.next.checked_add(1).context("文件操作编号溢出")?;
        self.requests
            .try_send(Request {
                id: self.next,
                operation,
                response: None,
            })
            .map_err(|_| anyhow::anyhow!("文件操作队列已满或连接已关闭"))?;
        Ok(self.next)
    }
    pub fn document_client(&self) -> super::document::Client {
        super::document::Client(self.requests.clone())
    }
    pub fn try_reply(&mut self) -> Option<Reply> {
        self.replies.try_recv().ok()
    }
    pub async fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.task.abort();
        let _ = (&mut self.task).await;
        if let Ok(mut state) = self.state.lock() {
            *state = Status::Exited;
        }
        wake(&self.notify);
    }
}
impl Drop for FileSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}
