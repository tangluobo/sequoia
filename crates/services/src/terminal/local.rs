use anyhow::{Result, bail};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex, mpsc},
};
use tomato_core::Connection;
#[cfg(all(test, windows))]
mod tests;
type OutputNotifier = Arc<dyn Fn() + Send + Sync>;
type SharedNotifier = Arc<Mutex<Option<OutputNotifier>>>;

pub(super) struct LocalTerminal {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub output: mpsc::Receiver<Vec<u8>>,
    notifier: SharedNotifier,
    // Close stdin and disconnect the bounded output queue before ClosePseudoConsole.
    // Otherwise a blocked reader or initial cursor query can deadlock its shutdown.
    master: Box<dyn portable_pty::MasterPty + Send>,
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
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
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
        Ok(Self {
            child,
            master: pair.master,
            writer,
            output,
            notifier,
        })
    }
    pub fn on_output(&self, callback: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut notifier) = self.notifier.lock() {
            *notifier = Some(Arc::new(callback));
        }
        notify(&self.notifier);
    }
    pub fn write(&self, text: &str) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("终端输入锁失效"))?;
        writer.write_all(text.as_bytes())?;
        writer.flush()?;
        Ok(())
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
    pub fn exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}
fn notify(notifier: &SharedNotifier) {
    let callback = notifier.lock().ok().and_then(|guard| guard.clone());
    if let Some(callback) = callback {
        callback();
    }
}
impl Drop for LocalTerminal {
    fn drop(&mut self) {
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
