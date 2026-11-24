//! Native asynchronous SSH. Credentials never enter process arguments or disk logs.
mod auth;
mod connect;
pub mod forwarding;
pub(crate) mod publish;
pub mod tools;
pub mod tunnel;
pub mod zmodem;
use super::{Prompt, Status};
use anyhow::{Context, Result, bail, ensure};
use russh::{ChannelMsg, client, keys};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tomato_core::Connection;
type Notifier = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
static KNOWN_HOSTS_LOCK: Mutex<()> = Mutex::new(());
enum Input {
    Data(Vec<u8>),
    Answers(Vec<String>),
    Transfer(zmodem::Choice),
    PrepareUpload,
}
pub struct RemoteTerminal {
    input: mpsc::Sender<Input>,
    size: tokio::sync::watch::Sender<(u16, u16)>,
    pub(super) output: mpsc::Receiver<Vec<u8>>,
    state: Arc<Mutex<Status>>,
    notifier: Notifier,
    task: tokio::task::JoinHandle<()>,
    requests: mpsc::Sender<tools::Request>,
    replies: mpsc::Receiver<tools::Reply>,
    next_request: u64,
    transfer: zmodem::Shared,
}
impl RemoteTerminal {
    pub(super) async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
    pub fn try_read(&mut self) -> Option<Vec<u8>> {
        self.output.try_recv().ok()
    }
    pub fn start(
        runtime: &tokio::runtime::Handle,
        connection: Connection,
        known_hosts: PathBuf,
    ) -> Result<Self> {
        let shell = connection.kind() != "SFTP";
        Self::start_mode(runtime, connection, known_hosts, shell)
    }
    pub fn start_mode(
        runtime: &tokio::runtime::Handle,
        connection: Connection,
        known_hosts: PathBuf,
        shell: bool,
    ) -> Result<Self> {
        ensure!(!connection.text("host").is_empty(), "SSH 主机不能为空");
        ensure!(
            !connection.text("username").is_empty(),
            "SSH 用户名不能为空"
        );
        let port = u16::try_from(connection.number("port"))?;
        ensure!(port > 0, "SSH 端口必须在 1–65535 之间");
        let (input, mut inputs) = mpsc::channel(32);
        let (output_send, output) = mpsc::channel(32);
        let (size, size_recv) = tokio::sync::watch::channel((32, 120));
        let (requests, request_recv) = mpsc::channel(8);
        let (reply_send, replies) = mpsc::channel(8);
        let transfer = Arc::new(Mutex::new(zmodem::TransferState::Idle));
        let transfer_task = transfer.clone();
        let state = Arc::new(Mutex::new(Status::Connecting));
        let notifier: Notifier = Arc::new(Mutex::new(None));
        let events = Events {
            state: state.clone(),
            notifier: notifier.clone(),
        };
        let task = runtime.spawn(async move {
            let result = run(
                connection,
                known_hosts,
                &mut inputs,
                output_send.clone(),
                size_recv,
                &events,
                ToolIo {
                    requests: request_recv,
                    replies: reply_send,
                    transfer: transfer_task.clone(),
                    shell,
                },
            )
            .await;
            if let Ok(mut transfer) = transfer_task.lock()
                && let Some(upload) = transfer.upload()
            {
                let error = "SSH 连接已关闭，传输未完成";
                *transfer = zmodem::TransferState::Failed(error.into());
                let _ = output_send.try_send(zmodem::terminal_notice(&format!(
                    "{}失败: {error}",
                    zmodem::direction(upload)
                )));
            }
            events.set(match result {
                Ok(()) => Status::Exited,
                Err(error) => Status::Failed(format!("{error:#}")),
            });
        });
        Ok(Self {
            input,
            size,
            output,
            state,
            notifier,
            task,
            requests,
            replies,
            next_request: 0,
            transfer,
        })
    }
    pub fn request(&mut self, operation: tools::Operation) -> Result<u64> {
        ensure!(self.status() == Status::Ready, "SSH 尚未连接");
        self.next_request = self
            .next_request
            .checked_add(1)
            .context("SSH 请求编号耗尽")?;
        let id = self.next_request;
        self.requests
            .try_send(tools::Request {
                id,
                operation,
                response: None,
            })
            .map_err(|_| anyhow::anyhow!("SSH 管理队列忙，请稍后重试"))?;
        Ok(id)
    }
    pub fn document_client(&self) -> crate::files::document::Client {
        crate::files::document::Client(self.requests.clone())
    }
    pub fn try_reply(&mut self) -> Option<tools::Reply> {
        self.replies.try_recv().ok()
    }
    pub fn transfer_state(&self) -> zmodem::TransferState {
        self.transfer
            .lock()
            .map(|s| s.clone())
            .unwrap_or(zmodem::TransferState::Idle)
    }
    pub fn prepare_upload(&self) -> Result<()> {
        self.input
            .try_send(Input::PrepareUpload)
            .map_err(|_| anyhow::anyhow!("SSH 传输队列忙"))
    }
    pub fn transfer_choice(&self, choice: zmodem::Choice) -> Result<()> {
        self.input
            .try_send(Input::Transfer(choice))
            .map_err(|_| anyhow::anyhow!("SSH 传输队列忙，请稍后重试"))
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut value) = self.notifier.lock() {
            *value = Some(Arc::new(callback));
        }
        Events {
            state: self.state.clone(),
            notifier: self.notifier.clone(),
        }
        .wake();
    }
    pub fn status(&self) -> Status {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| Status::Failed("SSH 状态锁失效".into()))
    }
    pub fn write(&self, text: &str) -> Result<()> {
        ensure!(self.status() == Status::Ready, "SSH 尚未连接");
        ensure!(
            text.len() <= 1024 * 1024,
            "单次终端输入最多 1 MiB，请分批粘贴"
        );
        self.input
            .try_send(Input::Data(text.as_bytes().to_vec()))
            .map_err(|_| anyhow::anyhow!("SSH 输入队列已满或会话已关闭"))?;
        Ok(())
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.size.send((rows, cols)).context("SSH 会话已关闭")
    }
    pub fn answer(&self, answers: Vec<String>) -> Result<()> {
        let previous = self.status();
        ensure!(
            matches!(previous, Status::Authentication { .. }),
            "SSH 当前没有等待输入的认证请求"
        );
        let events = Events {
            state: self.state.clone(),
            notifier: self.notifier.clone(),
        };
        events.set(Status::Connecting);
        if self.input.try_send(Input::Answers(answers)).is_err() {
            events.set(previous);
            bail!("SSH 认证队列已关闭");
        }
        Ok(())
    }
}
impl Drop for RemoteTerminal {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Events {
    state: Arc<Mutex<Status>>,
    notifier: Notifier,
}
impl Events {
    fn set(&self, state: Status) {
        if let Ok(mut target) = self.state.lock() {
            *target = state;
        }
        self.wake();
    }
    fn wake(&self) {
        let callback = self.notifier.lock().ok().and_then(|n| n.clone());
        if let Some(callback) = callback {
            callback();
        }
    }
    async fn ask(
        &self,
        title: String,
        prompts: Vec<Prompt>,
        input: &mut mpsc::Receiver<Input>,
    ) -> Result<Vec<String>> {
        self.set(Status::Authentication { title, prompts });
        while let Some(message) = input.recv().await {
            if let Input::Answers(values) = message {
                return Ok(values);
            }
        }
        bail!("SSH 认证已取消")
    }
}
struct Client {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    forwards: Option<mpsc::Sender<russh::Channel<client::Msg>>>,
}
struct SocketGuard(std::net::TcpStream);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}
impl client::Handler for Client {
    type Error = anyhow::Error;
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _address: &str,
        _port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<()> {
        if let Some(sender) = &self.forwards
            && let Ok(permit) = sender.try_reserve()
        {
            reply.accept().await;
            permit.send(channel);
        }
        Ok(())
    }
    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool> {
        let public = key.public_key();
        let host = self.host.clone();
        let port = self.port;
        let path = self.known_hosts.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let _guard = KNOWN_HOSTS_LOCK
                .lock()
                .map_err(|_| anyhow::anyhow!("SSH 主机记录锁失效"))?;
            if !keys::check_known_hosts_path(&host, port, &public, &path)
                .context("SSH 主机密钥与记录不一致")?
            {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                keys::known_hosts::learn_known_hosts_path(&host, port, &public, &path)?;
            }
            Ok(true)
        })
        .await?
    }
}
async fn run(
    connection: Connection,
    known_hosts: PathBuf,
    input: &mut mpsc::Receiver<Input>,
    output: mpsc::Sender<Vec<u8>>,
    mut size: tokio::sync::watch::Receiver<(u16, u16)>,
    events: &Events,
    mut tools: ToolIo,
) -> Result<()> {
    let connect::Connected {
        session,
        _parents,
        _socket_guard,
    } = connect::authenticated(&connection, known_hosts, input, events).await?;
    let mut transfer = zmodem::Bridge::new(tools.transfer.clone(), output);
    let mut channel = if !tools.shell {
        None
    } else {
        Some(timed(session.channel_open_session()).await?)
    };
    if let Some(channel) = &mut channel {
        let (rows, cols) = *size.borrow_and_update();
        channel
            .request_pty(
                true,
                "xterm-256color",
                u32::from(cols),
                u32::from(rows),
                0,
                0,
                // Match TerminalView's Backspace byte. Without VERASE, the
                // server's default (often DEL) leaves ^H in canonical reads
                // even when Readline at the shell prompt accepts it.
                &[(russh::Pty::VERASE, 8)],
            )
            .await?;
        acknowledge(channel, "终端尺寸协商", events, &mut transfer).await?;
        channel.request_shell(true).await?;
        acknowledge(channel, "Shell 启动", events, &mut transfer).await?;
    }
    let contexts = Arc::new(tools::Contexts {
        session: Arc::new(session),
        sftp: tokio::sync::OnceCell::new(),
        editors: Default::default(),
    });
    let mut jobs = tokio::task::JoinSet::new();

    let mut transfer_timer = tokio::time::interval(Duration::from_secs(2));
    transfer_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut edit_timer = tokio::time::interval(Duration::from_secs(1));
    edit_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    events.set(Status::Ready);
    loop {
        tokio::select! {
            request=tools.requests.recv()=>if let Some(request)=request {
                if jobs.len()>=4 {tools::request_reply(&tools.replies,request.id,request.response,Err(anyhow::anyhow!("最多同时执行四项 SSH 操作，请稍后重试"))).await;events.wake();}else{
                    let contexts=contexts.clone();let sender=tools.replies.clone();let notifier=events.notifier.clone();
                    jobs.spawn(async move {let result=contexts.execute(request.operation).await;tools::request_reply(&sender,request.id,request.response,result).await;let callback=notifier.lock().ok().and_then(|n|n.clone());if let Some(callback)=callback{callback();}});
                }
            },
            _=jobs.join_next(),if !jobs.is_empty()=>{},
            _=edit_timer.tick(),if contexts.editors.active() && jobs.len()<4=>{let contexts=contexts.clone();let sender=tools.replies.clone();let notifier=events.notifier.clone();jobs.spawn(async move {let result=contexts.sync_edits().await;let result=match result{Ok(None)=>return,Ok(Some(text))=>Ok(tools::Output::Done(text)),Err(error)=>Err(error)};tools::reply(&sender,0,result).await;if let Some(callback)=notifier.lock().ok().and_then(|n|n.clone()){callback();}});},
            _=tokio::time::sleep(Duration::from_millis(50)),if transfer.has_prefix()=>{transfer.flush_text(events).await?;},
            _=transfer_timer.tick(),if transfer.active()=>{if let Some(channel)=&channel && let Err(error)=transfer.tick(channel,events).await {transfer.cancel(channel,Some(format!("{error:#}")),events).await;}},
            message=input.recv()=>match message {
                Some(Input::PrepareUpload)=>transfer.prepare_upload(events).await?,
                Some(Input::Transfer(choice))=>if let Some(channel)=&channel && let Err(error)=transfer.choice(choice,channel,events).await {transfer.cancel(channel,Some(format!("{error:#}")),events).await;},
                Some(Input::Data(data))=>if let Some(channel)=&channel {if transfer.active(){if data.contains(&3){transfer.cancel(channel,None,events).await;channel.data(&[3][..]).await?;}}else{transfer.flush_text(events).await?;channel.data(data.as_slice()).await?;}},
                Some(Input::Answers(_))=>{},None=>break},
            changed=size.changed()=>{if changed.is_err(){break;}let(rows,cols)=*size.borrow_and_update();if let Some(channel)=&channel {channel.window_change(u32::from(cols),u32::from(rows),0,0).await?;}},
            message=async {match &mut channel {Some(channel)=>channel.wait().await,None=>std::future::pending().await}}=>match message {
                Some(ChannelMsg::Data{data}|ChannelMsg::ExtendedData{data,..})=>{if let Some(channel)=&channel && let Err(error)=transfer.feed(&data,channel,events).await {transfer.cancel(channel,Some(format!("{error:#}")),events).await;}},
                Some(ChannelMsg::Close)|None=>break,
                Some(ChannelMsg::ExitStatus{exit_status}) if exit_status!=0=>bail!("SSH shell 已退出，状态码 {exit_status}"),_=>{}
            }
        }
    }
    let _ = contexts
        .session
        .disconnect(russh::Disconnect::ByApplication, "", "")
        .await;
    Ok(())
}
struct ToolIo {
    requests: mpsc::Receiver<tools::Request>,
    replies: mpsc::Sender<tools::Reply>,
    transfer: zmodem::Shared,
    shell: bool,
}

async fn timed<T>(
    future: impl std::future::Future<Output = std::result::Result<T, russh::Error>>,
) -> Result<T> {
    Ok(tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .context("SSH 认证或通道请求超时")??)
}

async fn acknowledge(
    channel: &mut russh::Channel<client::Msg>,
    operation: &str,
    events: &Events,
    transfer: &mut zmodem::Bridge,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => return Ok(()),
                Some(ChannelMsg::Failure) => bail!("SSH 服务端拒绝{operation}"),
                Some(ChannelMsg::Close) | None => bail!("SSH 在{operation}时关闭了通道"),
                Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => {
                    transfer.feed(&data, channel, events).await?;
                }
                _ => {}
            }
        }
    })
    .await
    .with_context(|| format!("SSH {operation}超时"))?
}
