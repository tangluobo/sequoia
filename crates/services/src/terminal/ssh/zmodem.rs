//! Streaming ZMODEM adapter. File bytes never enter the VT parser or a full-file buffer.
use super::Events;
use anyhow::{Context, Result, ensure};
use russh::{Channel, client};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use zmodem2::{Action, Event, FileInfo, Position, Receiver, Sender};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferState {
    Idle,
    ChooseUpload,
    ChooseDirectory,
    Active {
        upload: bool,
        file: String,
        bytes: u64,
        total: Option<u64>,
    },
    Completed,
    Failed(String),
}
impl TransferState {
    pub fn upload(&self) -> Option<bool> {
        match self {
            Self::ChooseUpload => Some(true),
            Self::ChooseDirectory => Some(false),
            Self::Active { upload, .. } => Some(*upload),
            _ => None,
        }
    }
}

pub(super) fn direction(upload: bool) -> &'static str {
    if upload { "上传" } else { "下载" }
}

pub(super) fn terminal_notice(message: &str) -> Vec<u8> {
    let message: String = message
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    format!("\r\n[ZModem] {message}\r\n").into_bytes()
}

#[derive(Debug)]
pub enum Choice {
    Upload(Vec<PathBuf>),
    Download(PathBuf),
    Cancel,
}
pub(super) type Shared = Arc<Mutex<TransferState>>;
enum Mode {
    Idle,
    Choosing { upload: bool, buffer: Vec<u8> },
    Sending(Box<Send>),
    Receiving(Box<Receive>),
}
struct Send {
    machine: Sender,
    files: VecDeque<PathBuf>,
    file: Option<File>,
    next: bool,
    finished: bool,
    name: String,
    size: u64,
}
struct Receive {
    machine: Receiver,
    directory: PathBuf,
    file: Option<tempfile::NamedTempFile>,
    destination: Option<PathBuf>,
    count: u64,
    finished: bool,
}
pub(super) struct Bridge {
    mode: Mode,
    prefix: Vec<u8>,
    pub state: Shared,
    output: mpsc::Sender<Vec<u8>>,
    last_input: Instant,
    finish_marker: u8,
}
impl Bridge {
    pub fn new(state: Shared, output: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            mode: Mode::Idle,
            prefix: Vec::new(),
            state,
            output,
            last_input: Instant::now(),
            finish_marker: 0,
        }
    }
    pub async fn prepare_upload(&mut self, events: &Events) -> Result<()> {
        if !self.active() {
            self.mode = Mode::Choosing {
                upload: true,
                buffer: Vec::new(),
            };
            self.last_input = Instant::now();
            self.announce_choice(true, events).await?;
        }
        Ok(())
    }
    pub fn active(&self) -> bool {
        !matches!(self.mode, Mode::Idle)
    }
    pub fn has_prefix(&self) -> bool {
        matches!(self.mode, Mode::Idle) && !self.prefix.is_empty()
    }
    pub async fn flush_text(&mut self, events: &Events) -> Result<()> {
        self.normal(&self.prefix, events).await?;
        self.prefix.clear();
        Ok(())
    }
    fn update(&self, state: TransferState, events: &Events) {
        if let Ok(mut s) = self.state.lock() {
            *s = state;
        }
        events.wake();
    }
    async fn announce_choice(&self, upload: bool, events: &Events) -> Result<()> {
        let (state, message) = if upload {
            (
                TransferState::ChooseUpload,
                "检测到rz上传请求，请选择要上传的文件...",
            )
        } else {
            (
                TransferState::ChooseDirectory,
                "检测到sz下载请求，请选择保存目录...",
            )
        };
        self.update(state, events);
        self.notice(message, events).await
    }

    async fn notice(&self, message: &str, events: &Events) -> Result<()> {
        self.output
            .send(terminal_notice(message))
            .await
            .context("终端已关闭")?;
        events.wake();
        Ok(())
    }

    pub async fn cancel(
        &mut self,
        channel: &Channel<client::Msg>,
        error: Option<String>,
        events: &Events,
    ) {
        let message = self.state.lock().ok().and_then(|state| {
            let upload = state.upload()?;
            Some(match (&error, &*state) {
                (Some(error), _) => format!("{}失败: {error}", direction(upload)),
                (None, TransferState::ChooseUpload) => "未选择文件，取消上传".into(),
                (None, TransferState::ChooseDirectory) => "未选择保存目录，取消下载".into(),
                (None, _) => format!("{}已取消", direction(upload)),
            })
        });
        let _ = channel.data(&[0x18; 10][..]).await;
        self.mode = Mode::Idle;
        self.finish_marker = 0;
        self.prefix.clear();
        self.update(
            error
                .map(TransferState::Failed)
                .unwrap_or(TransferState::Idle),
            events,
        );
        if let Some(message) = message {
            let _ = self.notice(&message, events).await;
        }
    }
    pub async fn choice(
        &mut self,
        choice: Choice,
        channel: &Channel<client::Msg>,
        events: &Events,
    ) -> Result<()> {
        if matches!(choice, Choice::Cancel) {
            self.cancel(channel, None, events).await;
            return Ok(());
        }
        let Mode::Choosing { upload, buffer } = std::mem::replace(&mut self.mode, Mode::Idle)
        else {
            anyhow::bail!("当前没有等待选择的 ZModem 会话");
        };
        self.mode = match choice {
            Choice::Upload(files) => {
                ensure!(
                    upload && !files.is_empty() && files.len() <= 1000,
                    "请选择 1–1000 个上传文件"
                );
                let mut send = Send {
                    machine: Sender::new()?,
                    files: files.into(),
                    file: None,
                    next: false,
                    finished: false,
                    name: String::new(),
                    size: 0,
                };
                send.machine.set_streaming_window(8);
                send.next_file().await?;
                Mode::Sending(Box::new(send))
            }
            Choice::Download(directory) => {
                ensure!(!upload && directory.is_dir(), "请选择有效的下载目录");
                let mut machine = Receiver::new()?;
                machine.set_manual_file_accept(true);
                Mode::Receiving(Box::new(Receive {
                    machine,
                    directory,
                    file: None,
                    destination: None,
                    count: 0,
                    finished: false,
                }))
            }
            Choice::Cancel => unreachable!(),
        };
        let (file, total, message) = match &self.mode {
            Mode::Sending(send) => (send.name.clone(), Some(send.size), "正在上传文件...".into()),
            Mode::Receiving(receive) => (
                String::new(),
                None,
                format!("正在下载文件到: {}", receive.directory.display()),
            ),
            _ => unreachable!(),
        };
        self.update(
            TransferState::Active {
                upload,
                file,
                bytes: 0,
                total,
            },
            events,
        );
        self.notice(&message, events).await?;
        self.last_input = Instant::now();
        // The detected announcement is already the peer's initial header.
        // Answer it once rather than also sending our unsolicited startup header.
        if !buffer.is_empty() {
            match &mut self.mode {
                Mode::Sending(s) => {
                    if let Action::WriteWire(bytes) = s.machine.poll() {
                        let n = bytes.len();
                        s.machine.wire_written(n);
                    }
                }
                Mode::Receiving(r) => {
                    if let Action::WriteWire(bytes) = r.machine.poll() {
                        let n = bytes.len();
                        r.machine.wire_written(n);
                    }
                }
                _ => {}
            }
        }
        self.pump(channel, events).await?;
        self.protocol(&buffer, channel, events).await?;
        Ok(())
    }
    pub async fn feed(
        &mut self,
        data: &[u8],
        channel: &Channel<client::Msg>,
        events: &Events,
    ) -> Result<()> {
        self.last_input = Instant::now();
        if let Mode::Choosing { buffer, .. } = &mut self.mode {
            ensure!(
                buffer.len() + data.len() <= 64 * 1024,
                "选择文件期间 ZModem 缓冲超出限制"
            );
            buffer.extend_from_slice(data);
            return Ok(());
        }
        let data = if !matches!(self.mode, Mode::Idle) {
            let consumed = self.protocol(data, channel, events).await?;
            if consumed == data.len() {
                return Ok(());
            }
            &data[consumed..]
        } else {
            data
        };
        let mut skip = 0;
        while self.finish_marker > 0 && skip < data.len() {
            match data[skip] {
                b'O' => {
                    self.finish_marker -= 1;
                    skip += 1;
                }
                10 | 13 | 17 => skip += 1,
                _ => {
                    self.finish_marker = 0;
                }
            }
        }
        let data = &data[skip..];
        self.prefix.extend_from_slice(data);
        if let Some(index) = self
            .prefix
            .windows(6)
            .position(|p| p == b"**\x18B00" || p == b"**\x18B01")
        {
            let upload = self.prefix[index + 5] == b'1';
            self.normal(&self.prefix[..index], events).await?;
            let buffer = self.prefix.split_off(index);
            self.prefix.clear();
            self.mode = Mode::Choosing { upload, buffer };
            self.announce_choice(upload, events).await?;
        } else {
            let keep = (1..=5)
                .rev()
                .find(|n| self.prefix.ends_with(&b"**\x18B0"[..*n]))
                .unwrap_or(0);
            let emit = self.prefix.len() - keep;
            self.normal(&self.prefix[..emit], events).await?;
            self.prefix.drain(..emit);
        }
        Ok(())
    }
    async fn normal(&self, data: &[u8], events: &Events) -> Result<()> {
        for chunk in data.chunks(4096) {
            self.output
                .send(chunk.to_vec())
                .await
                .context("终端已关闭")?;
            events.wake();
        }
        Ok(())
    }
    async fn protocol(
        &mut self,
        data: &[u8],
        channel: &Channel<client::Msg>,
        events: &Events,
    ) -> Result<usize> {
        let mut offset = 0;
        while offset < data.len() && self.active() {
            self.pump(channel, events).await?;
            if !self.active() {
                break;
            }
            let consumed = match &mut self.mode {
                Mode::Sending(s) => s.machine.submit_wire(&data[offset..])?,
                Mode::Receiving(r) => r.machine.submit_wire(&data[offset..])?,
                _ => 0,
            };
            ensure!(consumed > 0, "ZModem 状态机未能消费输入");
            offset += consumed;
            self.pump(channel, events).await?;
        }
        Ok(offset)
    }
    pub async fn tick(&mut self, channel: &Channel<client::Msg>, events: &Events) -> Result<()> {
        let limit = if matches!(self.mode, Mode::Choosing { .. }) {
            120
        } else {
            60
        };
        ensure!(
            self.last_input.elapsed() < Duration::from_secs(limit),
            "ZModem 等待超时"
        );
        if self.last_input.elapsed() >= Duration::from_secs(2) {
            match &mut self.mode {
                Mode::Sending(s) => s.machine.timeout()?,
                Mode::Receiving(r) => r.machine.timeout()?,
                _ => {}
            }
            self.pump(channel, events).await?;
        }
        Ok(())
    }
    async fn pump(&mut self, channel: &Channel<client::Msg>, events: &Events) -> Result<()> {
        loop {
            let state = &self.state;
            let done = match &mut self.mode {
                Mode::Sending(s) => match s.machine.poll() {
                    Action::WriteWire(bytes) => {
                        let n = bytes.len();
                        channel.data(bytes).await?;
                        s.machine.wire_written(n);
                        false
                    }
                    Action::ReadFile { offset, max_len } => {
                        let mut file = s.file.take().context("上传文件未打开")?;
                        let (file, bytes) =
                            tokio::task::spawn_blocking(move || -> std::io::Result<_> {
                                file.seek(SeekFrom::Start(u64::from(offset.get())))?;
                                let mut bytes = vec![0; max_len.min(64 * 1024)];
                                let n = file.read(&mut bytes)?;
                                bytes.truncate(n);
                                Ok((file, bytes))
                            })
                            .await??;
                        s.file = Some(file);
                        let n = bytes.len();
                        s.machine.submit_file(&bytes)?;
                        if let Ok(mut target) = state.lock()
                            && let TransferState::Active { bytes, .. } = &mut *target
                        {
                            *bytes = u64::from(offset.get()) + n as u64;
                        }
                        false
                    }
                    Action::Event(Event::FileCompleted) => {
                        s.next = true;
                        false
                    }
                    Action::Event(Event::SessionCompleted) => {
                        s.finished = true;
                        false
                    }
                    Action::Event(Event::Aborted) => anyhow::bail!("远端取消了 ZModem 传输"),
                    Action::Idle => {
                        if s.finished {
                            true
                        } else if s.next {
                            s.next = false;
                            s.next_file().await?;
                            if s.file.is_some()
                                && let Ok(mut target) = state.lock()
                            {
                                *target = TransferState::Active {
                                    upload: true,
                                    file: s.name.clone(),
                                    bytes: 0,
                                    total: Some(s.size),
                                };
                            }
                            false
                        } else {
                            return Ok(());
                        }
                    }
                    _ => false,
                },
                Mode::Receiving(r) => match r.machine.poll() {
                    Action::WriteWire(bytes) => {
                        let n = bytes.len();
                        channel.data(bytes).await?;
                        r.machine.wire_written(n);
                        false
                    }
                    Action::Event(Event::FileStarted(info)) => {
                        let name = std::str::from_utf8(info.name)
                            .context("ZModem 文件名不是 UTF-8")?
                            .to_string();
                        ensure!(safe_name(&name), "ZModem 文件名包含路径或非法字符");
                        let destination = r.directory.join(&name);
                        let directory = r.directory.clone();
                        let file = tokio::task::spawn_blocking(move || {
                            tempfile::NamedTempFile::new_in(directory)
                        })
                        .await??;
                        r.file = Some(file);
                        r.destination = Some(destination);
                        r.count = 0;
                        if let Ok(mut target) = state.lock() {
                            *target = TransferState::Active {
                                upload: false,
                                file: name,
                                bytes: 0,
                                total: info.size.map(|n| u64::from(n.get())),
                            };
                        }
                        r.machine.accept_file_at(0)?;
                        false
                    }
                    Action::WriteFile(bytes) => {
                        let mut file = r.file.take().context("下载文件未打开")?;
                        let bytes = bytes.to_vec();
                        let n = bytes.len();
                        r.file = Some(
                            tokio::task::spawn_blocking(move || -> std::io::Result<_> {
                                file.write_all(&bytes)?;
                                Ok(file)
                            })
                            .await??,
                        );
                        r.machine.file_written(n)?;
                        r.count += n as u64;
                        if let Ok(mut target) = state.lock()
                            && let TransferState::Active { bytes, .. } = &mut *target
                        {
                            *bytes = r.count;
                        }
                        false
                    }
                    Action::Event(Event::FileCompleted) => {
                        let file = r.file.take().context("下载文件未打开")?;
                        let path = r.destination.take().context("下载路径缺失")?;
                        tokio::task::spawn_blocking(move || -> Result<()> {
                            file.as_file().sync_all()?;
                            file.persist(path).map_err(|e| e.error)?;
                            Ok(())
                        })
                        .await??;
                        false
                    }
                    Action::Event(Event::SessionCompleted) => {
                        r.finished = true;
                        self.finish_marker = 2;
                        false
                    }
                    Action::Event(Event::Aborted) => anyhow::bail!("远端取消了 ZModem 传输"),
                    Action::Idle => {
                        if r.finished {
                            true
                        } else {
                            return Ok(());
                        }
                    }
                    _ => false,
                },
                _ => return Ok(()),
            };
            if done {
                let upload = matches!(self.mode, Mode::Sending(_));
                self.mode = Mode::Idle;
                self.notice(&format!("{}完成", direction(upload)), events)
                    .await?;
                self.update(TransferState::Completed, events);
                return Ok(());
            }
        }
    }
}
impl Send {
    async fn next_file(&mut self) -> Result<()> {
        let Some(path) = self.files.pop_front() else {
            self.file = None;
            self.machine.finish()?;
            return Ok(());
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("上传文件名无效")?
            .to_string();
        ensure!(safe_name(&name), "上传文件名包含不支持的字符");
        let (file, size) = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
            let file = File::open(path)?;
            let size = file.metadata()?.len();
            Ok((file, size))
        })
        .await??;
        ensure!(
            size <= u64::from(u32::MAX),
            "ZModem 文件最大为 4 GiB，请使用 SFTP 传输更大的文件"
        );
        self.machine.start_file(FileInfo::new(
            name.as_bytes(),
            Some(Position::new(size as u32)),
        ))?;
        self.file = Some(file);
        self.name = name;
        self.size = size;
        Ok(())
    }
}
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0', '\r', '\n'])
        && !name.ends_with(['.', ' '])
        && Path::new(name).file_name().is_some_and(|n| n == name)
}
