use super::Status;
use anyhow::{Context, Result, bail, ensure};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use tomato_core::Connection;
#[cfg(all(test, windows))]
mod tests;
type OutputNotifier = Arc<dyn Fn() + Send + Sync>;
type SharedNotifier = Arc<Mutex<Option<OutputNotifier>>>;

pub(super) struct LocalTerminal {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    input: Option<mpsc::SyncSender<Input>>,
    pub output: mpsc::Receiver<Vec<u8>>,
    notifier: SharedNotifier,
    controls: Arc<Controls>,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum Input {
    Data(Vec<u8>),
    Resize,
}

#[derive(Default)]
struct Controls {
    size: Mutex<Option<PtySize>>,
    failure: Mutex<Option<String>>,
    stopping: AtomicBool,
}

impl LocalTerminal {
    pub fn start(connection: &Connection) -> Result<Self> {
        let system = native_pty_system();
        let pair = system.openpty(PtySize {
            rows: 32,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let command = command(connection)?;
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let master = pair.master;
        let (send, output) = mpsc::sync_channel(32);
        let notifier: SharedNotifier = Arc::new(Mutex::new(None));
        let reader_notifier = notifier.clone();
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            let mut forwarding = true;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if forwarding {
                            forwarding = send.send(buffer[..count].to_vec()).is_ok();
                            if forwarding {
                                notify(&reader_notifier);
                            }
                        }
                        // ConPTY can flush final output during ClosePseudoConsole.
                        // Keep draining after the UI receiver is dropped, without
                        // allocating or retaining those discarded output chunks.
                    }
                }
            }
            notify(&reader_notifier);
        });
        let (input, receive) = mpsc::sync_channel(8);
        let controls = Arc::new(Controls::default());
        let worker_controls = controls.clone();
        let worker_notifier = notifier.clone();
        let worker = std::thread::spawn(move || {
            let result = run_input(&receive, &worker_controls, &*master, &mut *writer);
            drop(receive);
            if let Err(error) = result {
                if let Ok(mut failure) = worker_controls.failure.lock() {
                    *failure = Some(format!("{error:#}"));
                }
                notify(&worker_notifier);
            }
            // ConPTY can flush output during close. Drop stdin first and let
            // the reader keep draining when the owning tab drops its receiver.
            drop(writer);
            drop(master);
        });
        Ok(Self {
            child,
            input: Some(input),
            output,
            notifier,
            controls,
            worker: Some(worker),
        })
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut notifier) = self.notifier.lock() {
            *notifier = Some(Arc::new(callback));
        }
        notify(&self.notifier);
    }
    pub fn write(&self, text: &str) -> Result<()> {
        ensure!(
            text.len() <= 1024 * 1024,
            "单次终端输入最多 1 MiB，请分批粘贴"
        );
        self.input
            .as_ref()
            .context("本地终端已关闭")?
            .try_send(Input::Data(text.as_bytes().to_vec()))
            .map_err(|_| anyhow::anyhow!("本地终端输入队列已满或会话已关闭"))?;
        Ok(())
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        *self
            .controls
            .size
            .lock()
            .map_err(|_| anyhow::anyhow!("终端尺寸锁失效"))? = Some(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        match self
            .input
            .as_ref()
            .context("本地终端已关闭")?
            .try_send(Input::Resize)
        {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => bail!("本地终端已关闭"),
        }
    }
    pub fn status(&mut self) -> Status {
        if self.child.try_wait().ok().flatten().is_some() {
            Status::Exited
        } else if let Some(error) = self.controls.failure.lock().ok().and_then(|f| f.clone()) {
            Status::Failed(error)
        } else {
            Status::Ready
        }
    }

    pub(super) fn shutdown(mut self) {
        let worker = self.worker.take();
        drop(self);
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

fn run_input(
    receive: &mpsc::Receiver<Input>,
    controls: &Controls,
    master: &dyn portable_pty::MasterPty,
    writer: &mut dyn Write,
) -> Result<()> {
    while let Ok(input) = receive.recv() {
        if controls.stopping.load(Ordering::Acquire) {
            break;
        }
        let size = controls
            .size
            .lock()
            .map_err(|_| anyhow::anyhow!("终端尺寸锁失效"))?
            .take();
        if let Some(size) = size {
            master.resize(size).context("调整本地终端尺寸失败")?;
        }
        if let Input::Data(bytes) = input {
            writer.write_all(&bytes).context("写入本地终端失败")?;
            writer.flush().context("刷新本地终端输入失败")?;
        }
    }
    Ok(())
}

fn notify(notifier: &SharedNotifier) {
    let callback = notifier.lock().ok().and_then(|guard| guard.clone());
    if let Some(callback) = callback {
        callback();
    }
}
impl Drop for LocalTerminal {
    fn drop(&mut self) {
        self.controls.stopping.store(true, Ordering::Release);
        self.input.take();
        if let Ok(mut notifier) = self.notifier.lock() {
            *notifier = None;
        }
        let _ = self.child.kill();
    }
}

fn command(connection: &Connection) -> Result<CommandBuilder> {
    match connection.kind() {
        "LOCAL_TERMINAL" => {
            let shell = if cfg!(windows) {
                if connection.text("terminalType") == "cmd" {
                    "cmd.exe".into()
                } else {
                    "powershell.exe".into()
                }
            } else {
                std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
            };
            let mut command = CommandBuilder::new(shell);
            command.env("TERM", "xterm-256color");
            if !connection.text("localDirectoryPath").is_empty() {
                command.cwd(connection.text("localDirectoryPath"));
            }
            Ok(command)
        }
        "SFTP" => {
            let mut command = CommandBuilder::new("sftp");
            command.arg("-P");
            command.arg(connection.number("port").to_string());
            if let Some(keys) = connection
                .options
                .get("privateKeyPaths")
                .and_then(serde_json::Value::as_array)
            {
                for key in keys.iter().filter_map(serde_json::Value::as_str) {
                    command.arg("-i");
                    command.arg(key);
                }
            }
            let host = connection.text("host");
            if host.starts_with('-') || host.is_empty() {
                bail!("无效 SSH 主机");
            }
            let user = connection.text("username");
            command.arg("--");
            command.arg(if user.is_empty() {
                host.to_string()
            } else {
                format!("{user}@{host}")
            });
            Ok(command)
        }
        _ => bail!("此连接类型不是终端"),
    }
}
