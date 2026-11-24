//! End-to-end SSH workspace fixtures. All sockets and files are disposable.
use anyhow::{Context, Result, ensure};
use russh::{
    Channel, ChannelId,
    keys::{PrivateKey, ssh_key::private::Ed25519Keypair},
    server,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tomato_core::Connection;
#[path = "ssh_workspace/docker.rs"]
mod docker;
#[path = "ssh_workspace/documents.rs"]
mod documents;
#[path = "ssh_workspace/storage_panel.rs"]
mod storage_panel;
#[path = "ssh_workspace/storage_tunnel.rs"]
mod storage_tunnel;
#[path = "ssh_workspace/upload.rs"]
mod upload;
use tomato_services::terminal::{
    Status,
    ssh::{
        RemoteTerminal,
        tools::{Operation, Output, Section},
        zmodem::{Choice, TransferState},
    },
};

struct Java(
    Child,
    Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    Arc<std::sync::atomic::AtomicBool>,
);
impl Drop for Java {
    fn drop(&mut self) {
        self.1.take();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[derive(Clone)]
enum Mode {
    Shell,
    Monitor(Arc<std::sync::atomic::AtomicU64>),
    Docker(Arc<std::sync::Mutex<docker::Peer>>),
    Storage(Arc<std::sync::Mutex<storage_panel::Peer>>),
    Readline(PathBuf),
    JavaReceive(PathBuf),
    JavaSend(Vec<PathBuf>),
    NativeSend(Vec<PathBuf>),
}
struct Handler {
    mode: Mode,
    child: Option<Java>,
    readline: Option<Readline>,
    channels: HashMap<ChannelId, Channel<server::Msg>>,
    root: PathBuf,
    shell: Option<ChannelId>,
    tasks: tokio::task::JoinSet<()>,
}
struct Readline {
    child: Arc<std::sync::Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    input: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    monitor: tokio::task::JoinHandle<()>,
}
impl Drop for Readline {
    fn drop(&mut self) {
        self.monitor.abort();
        self.input.take();
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.master.take();
    }
}
impl Readline {
    fn start(
        executable: &Path,
        root: &Path,
        id: ChannelId,
        session: &server::Session,
    ) -> Result<Self> {
        let pair = portable_pty::native_pty_system().openpty(portable_pty::PtySize {
            rows: 32,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = portable_pty::CommandBuilder::new(executable);
        command.args(["--noprofile", "--norc", "-i"]);
        command.cwd(root);
        for (name, value) in [
            ("TERM", "xterm-256color"),
            ("PS1", "fixture$ "),
            ("PROMPT_COMMAND", ""),
            ("INPUTRC", ".inputrc"),
            ("HISTFILE", "/dev/null"),
            ("BASH_ENV", ""),
        ] {
            command.env(name, value);
        }
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (input, receive) = std::sync::mpsc::sync_channel::<Vec<u8>>(32);
        std::thread::spawn(move || {
            while let Ok(bytes) = receive.recv() {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let handle = session.handle();
        let runtime = tokio::runtime::Handle::current();
        let cursor_reply = input.clone();
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            let mut cursor_query = 0;
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                // ConPTY inherits a cursor position before starting the child.
                // This disposable PTY has no parent terminal emulator to answer it.
                for &byte in &bytes[..count] {
                    cursor_query = if byte == b"\x1b[6n"[cursor_query] {
                        cursor_query + 1
                    } else {
                        usize::from(byte == 0x1b)
                    };
                    if cursor_query == 4 {
                        let _ = cursor_reply.try_send(b"\x1b[1;1R".to_vec());
                        cursor_query = 0;
                    }
                }
                if runtime
                    .block_on(handle.data(id, bytes[..count].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
            let _ = runtime.block_on(handle.eof(id));
            let _ = runtime.block_on(handle.close(id));
        });
        let child = Arc::new(std::sync::Mutex::new(pair.slave.spawn_command(command)?));
        drop(pair.slave);
        let child_state = child.clone();
        let handle = session.handle();
        // ConPTY's output pipe can remain open until the master is closed.
        // Reap the actual process rather than depending on that pipe's EOF.
        let monitor = tokio::spawn(async move {
            loop {
                let exited = child_state
                    .lock()
                    .ok()
                    .and_then(|mut child| child.try_wait().ok().flatten())
                    .is_some();
                if exited {
                    let _ = handle.eof(id).await;
                    let _ = handle.close(id).await;
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        Ok(Self {
            child,
            master: Some(pair.master),
            input: Some(input),
            monitor,
        })
    }
}
impl server::Handler for Handler {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth> {
        Ok(if user == "fixture" && password == "fixture-only" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<()> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<()> {
        ensure!(
            host.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()),
            "Only fixture loopback forwarding is permitted"
        );
        let mut socket = tokio::net::TcpStream::connect((host, u16::try_from(port)?)).await?;
        reply.accept().await;
        self.tasks.spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
        });
        Ok(())
    }
    async fn pty_request(
        &mut self,
        id: ChannelId,
        _: &str,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<()> {
        ensure!(
            modes.contains(&(russh::Pty::VERASE, 8)),
            "SSH erase character differs from the terminal key mapping"
        );
        session.channel_success(id)?;
        Ok(())
    }
    async fn shell_request(&mut self, id: ChannelId, session: &mut server::Session) -> Result<()> {
        let channel = self.channels.remove(&id);
        if let Mode::NativeSend(files) = &self.mode {
            session.channel_success(id)?;
            let stream = channel.context("Native fixture shell")?.into_stream();
            let files = files.clone();
            self.tasks.spawn(async move {
                if let Err(error) = native_sender(stream, files).await {
                    eprintln!("Native ZMODEM peer: {error:#}");
                }
            });
            return Ok(());
        }
        self.shell = Some(id);
        session.channel_success(id)?;
        if let Mode::Readline(executable) = &self.mode {
            self.readline = Some(Readline::start(executable, &self.root, id, session)?);
            return Ok(());
        }
        if matches!(
            self.mode,
            Mode::Shell | Mode::Monitor(_) | Mode::Docker(_) | Mode::Storage(_)
        ) {
            if std::env::var_os("TOMATO_TEST_TERMINAL_LAYOUT").is_some() {
                session.data(
                    id,
                    b"\x1b[7m    PID USER      PR  NI    VIRT    RES\x1b[K\x1b[0m\r\n      1 root      20   0   25604  13128\r\n\x1b[32mroot@fixture:~\x1b[0m# "
                        .to_vec(),
                )?;
                return Ok(());
            }
            if std::env::var_os("TOMATO_TEST_TERMINAL_HISTORY").is_some() {
                let lines = (0..150)
                    .map(|index| format!("history {index:03} 中文输出\r\n"))
                    .collect::<String>();
                session.data(id, lines.into_bytes())?;
            }
            session.data(id, b"fixture$ ".to_vec())?;
            return Ok(());
        }
        let classes = java_classes()?;
        let mut command = std::process::Command::new(java_tool("java"));
        command
            .args(["-Dfile.encoding=UTF-8", "-cp"])
            .arg(classes)
            .arg("LegacyZmodemPeer");
        match &self.mode {
            Mode::JavaReceive(dir) => {
                command.arg("receive").arg(dir);
            }
            Mode::JavaSend(files) => {
                command.arg("send").args(files);
            }
            _ => {}
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let mut input = child.stdin.take().context("Java stdin")?;
        let mut output = child.stdout.take().context("Java stdout")?;
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(32);
        std::thread::spawn(move || {
            while let Ok(bytes) = receiver.recv() {
                if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                    break;
                }
            }
        });
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = done.clone();
        let handle = session.handle();
        let runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let mut buffer = [0; 2048];
            while let Ok(n) = output.read(&mut buffer) {
                if n == 0 {
                    finished.store(true, std::sync::atomic::Ordering::Release);
                    let _ = runtime.block_on(handle.data(id, b"\r\nfixture$ ".to_vec()));
                    break;
                }
                if runtime
                    .block_on(handle.data(id, buffer[..n].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.child = Some(Java(child, Some(sender), done));
        Ok(())
    }
    async fn data(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        // The Java receive loop returns at ZFIN; emulate the remote command's
        // final OO consumption so those bytes cannot become shell input.
        if matches!(self.mode, Mode::JavaReceive(_)) && data == b"OO" {
            return Ok(());
        }
        if self.shell != Some(id) {
            return Ok(());
        }
        if let Mode::Docker(peer) = &self.mode {
            let mut peer = peer.lock().unwrap();
            ensure!(
                peer.shell_input.len() + data.len() <= 4096,
                "Fixture input bound exceeded"
            );
            peer.shell_input.extend_from_slice(data);
        }
        if let Some(readline) = &self.readline {
            readline
                .input
                .as_ref()
                .context("Readline fixture input closed")?
                .try_send(data.to_vec())
                .context("Readline fixture input queue")?;
            return Ok(());
        }
        if let Some(Java(child, _, done)) = &mut self.child
            && (done.load(std::sync::atomic::Ordering::Acquire) || child.try_wait()?.is_some())
        {
            self.child = None;
        }
        if let Some(Java(_, Some(sender), _)) = &self.child {
            sender
                .try_send(data.to_vec())
                .context("Reference peer input queue")?;
        } else {
            session.data(id, data.to_vec())?;
        }
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        self.channels.remove(&id);
        session.channel_success(id)?;
        let command = std::str::from_utf8(data)?;
        if let Mode::Docker(peer) = &self.mode {
            return docker::serve(peer, id, command, session);
        }
        if let Mode::Storage(peer) = &self.mode {
            return storage_panel::serve(peer, id, command, session);
        }
        if matches!(self.mode, Mode::Monitor(_)) {
            eprintln!("Owned monitor peer exec: {command}");
        }
        if let Mode::Monitor(counter) = &self.mode
            && command.contains("/proc/stat")
        {
            ensure!(
                command.contains("/proc/stat") && command.contains("/sys/block/"),
                "Unexpected monitor query"
            );
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let output = format!(
                "cpu {} 0 100 {} 0 0 0 0\n__SEP__\nMemTotal: 1000 kB\nMemAvailable: 400 kB\n__SEP__\neth0: {} 0 0 0 0 0 0 0 {}\n__SEP__\n8 0 sda 1 0 {} 0 1 0 {} 0\n8 1 sda1 1 0 {} 0 1 0 {} 0\n__SEP__\nsda\n",
                100 + n * 20,
                800 + n * 80,
                n * 4096,
                n * 2048,
                n * 100,
                n * 200,
                n * 100,
                n * 200
            );
            session.data(id, output.into_bytes())?;
            session.exit_status_request(id, 0)?;
            session.eof(id)?;
            session.close(id)?;
            return Ok(());
        }
        let output = if command.contains("docker ps") {
            "abc123|alpine:3|web|Up 2 hours|0.0.0.0:8080->80/tcp|2026-10-15\n"
        } else if command.contains("ss -lnpH") {
            "tcp LISTEN 0 4096 [::]:22 [::]:* users:((\"sshd\",pid=42,fd=3))\nudp UNCONN 0 0 0.0.0.0:53 0.0.0.0:* users:((\"dns\",pid=91,fd=3))\n__TOMATO_FIREWALL__\nports: 22/tcp\n"
        } else {
            "fixture command completed\n"
        };
        session.data(id, output.as_bytes().to_vec())?;
        session.exit_status_request(id, 0)?;
        session.eof(id)?;
        session.close(id)?;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<()> {
        if name != "sftp" {
            session.channel_failure(id)?;
            return Ok(());
        }
        let channel = self.channels.remove(&id).context("SFTP channel")?;
        session.channel_success(id)?;
        let fs = Fs::new(self.root.clone());
        self.tasks.spawn(async move {
            russh_sftp::server::run(channel.into_stream(), fs).await;
        });
        Ok(())
    }
}
struct Fixture {
    root: tempfile::TempDir,
    port: u16,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn start(mode: Mode) -> Result<Self> {
        if matches!(mode, Mode::JavaReceive(_) | Mode::JavaSend(_)) {
            tokio::task::spawn_blocking(java_classes).await??;
        }
        let root = tempfile::tempdir()?;
        let path = root.path().to_path_buf();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let config = Arc::new(server::Config {
            keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[101; 32]))],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {socket=listener.accept()=>{let Ok((socket,_))=socket else{break;};let config=config.clone();let handler=Handler {mode:mode.clone(),child:None,readline:None,channels:HashMap::new(),root:path.clone(),shell:None,tasks:tokio::task::JoinSet::new()};tasks.spawn(async move {if let Ok(session)=server::run_stream(config,socket,handler).await {let _=session.await;}});},_=tasks.join_next(),if !tasks.is_empty()=>{}}
            }
        });
        Ok(Self { root, port, task })
    }
    async fn terminal(&self, kind: &str) -> Result<RemoteTerminal> {
        let mut connection = Connection::new(kind);
        connection.set("port", self.port);
        connection.set("username", "fixture");
        connection.set("password", "fixture-only");
        let terminal = RemoteTerminal::start(
            &tokio::runtime::Handle::current(),
            connection,
            self.root.path().join("known_hosts"),
        )?;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match terminal.status() {
                    Status::Ready => return Ok::<_, anyhow::Error>(()),
                    Status::Failed(e) => anyhow::bail!(e),
                    _ => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await??;
        Ok(terminal)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TOMATO_TEST_BASH pointing to an installed Bash; isolated PTY and loopback SSH"]
async fn real_readline_completes_tab_and_erases_backspace() -> Result<()> {
    let executable =
        PathBuf::from(std::env::var_os("TOMATO_TEST_BASH").context("Missing Bash path")?);
    ensure!(executable.is_file(), "Bash executable not found");
    let fixture = Fixture::start(Mode::Readline(executable)).await?;
    std::fs::write(
        fixture.root.path().join(".inputrc"),
        "set editing-mode emacs\nset bell-style none\n",
    )?;
    std::fs::write(
        fixture.root.path().join("completion-target.txt"),
        "fixture-only",
    )?;
    let mut terminal = fixture.terminal("SSH").await?;
    shell_output(&mut terminal, "fixture$").await?;
    terminal.write("printf 'RESULT:%s\\n' completion-t\t\r")?;
    shell_output(&mut terminal, "RESULT:completion-target.txt").await?;
    terminal.write("printf 'ERASE:<%s>\\n' OKx\x08\r")?;
    shell_output(&mut terminal, "ERASE:<OK>").await?;
    // Windows ConPTY treats Ctrl+C as a console control event; MSYS Bash can
    // exit instead of behaving like a Unix SSH server. Keep that host-specific
    // behavior out of this completion fixture. The raw SSH bytes are tested
    // separately; a Unix PTY also verifies cancellation here.
    #[cfg(unix)]
    {
        terminal.write("touch forbidden-marker\x03")?;
        shell_output(&mut terminal, "fixture$").await?;
        terminal.write("printf 'INTERRUPT:%s\\n' OK\r")?;
        shell_output(&mut terminal, "INTERRUPT:OK").await?;
        ensure!(
            !fixture.root.path().join("forbidden-marker").exists(),
            "Ctrl+C failed to cancel the draft command"
        );
    }
    terminal.write("exit\r")?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match terminal.status() {
                Status::Exited => return Ok::<_, anyhow::Error>(()),
                Status::Failed(error) => anyhow::bail!(error),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await??;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TOMATO_TEST_BASH; real Bash script reads through isolated PTY and loopback SSH"]
async fn script_account_hidden_password_and_readline_delete_are_editable() -> Result<()> {
    let executable =
        PathBuf::from(std::env::var_os("TOMATO_TEST_BASH").context("Missing Bash path")?);
    ensure!(executable.is_file(), "Bash executable not found");
    let fixture = Fixture::start(Mode::Readline(executable)).await?;
    std::fs::write(
        fixture.root.path().join(".inputrc"),
        "set editing-mode emacs\nset bell-style none\n",
    )?;
    std::fs::write(
        fixture.root.path().join("input.sh"),
        "\"${BASH%/*}/stty\" erase '^H'\nprintf 'ACCOUNT_READY:'\nIFS= read -r account\nprintf 'HIDDEN_READY:'\nIFS= read -rs secret\nprintf '\\nEDIT_READY:'\nIFS= read -er edited\nprintf '%s\\n' \"$account\" \"$secret\" \"$edited\" > input-result.txt\nprintf '\\nINPUT_FINISHED\\n'\n",
    )?;
    let mut terminal = fixture.terminal("SSH").await?;
    shell_output(&mut terminal, "fixture$").await?;
    terminal.write("\"$BASH\" ./input.sh\r")?;
    wait_output(&mut terminal, "ACCOUNT_READY:", false).await?;
    terminal.write("fixture-accountXX\x08\x08\r")?;
    wait_output(&mut terminal, "HIDDEN_READY:", false).await?;
    terminal.write("fixture-secretXX\x08\x08\r")?;
    let hidden = wait_output(&mut terminal, "EDIT_READY:", false).await?;
    ensure!(
        !hidden.contains("fixture-secret"),
        "Hidden input was echoed"
    );
    terminal.write("abXc\x1b[D\x1b[D\x1b[3~\x1b[FZ\x08\r")?;
    shell_output(&mut terminal, "INPUT_FINISHED").await?;
    let result = std::fs::read_to_string(fixture.root.path().join("input-result.txt"))?;
    assert_eq!(
        result.replace("\r\n", "\n"),
        "fixture-account\nfixture-secret\nabc\n"
    );
    Ok(())
}
async fn shell_output(terminal: &mut RemoteTerminal, needle: &str) -> Result<()> {
    wait_output(terminal, needle, true).await.map(|_| ())
}
async fn wait_output(terminal: &mut RemoteTerminal, needle: &str, prompt: bool) -> Result<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    loop {
        while let Some(bytes) = terminal.try_read() {
            output.extend(bytes);
        }
        ensure!(output.len() <= 64 * 1024, "Readline fixture output limit");
        let display = String::from_utf8_lossy(&output);
        if let Some(index) = display.find(needle)
            && (!prompt
                || needle == "fixture$"
                || display[index + needle.len()..].contains("fixture$"))
        {
            return Ok(display.into_owned());
        }
        if let Status::Failed(error) = terminal.status() {
            anyhow::bail!(error);
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Readline fixture expected {needle:?}, received {:?}",
            String::from_utf8_lossy(&output)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn request(terminal: &mut RemoteTerminal, operation: Operation) -> Result<Output> {
    let id = terminal.request(operation)?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(reply) = terminal.try_reply() {
                if reply.id == 0 {
                    continue;
                }
                ensure!(reply.id == id, "Unexpected reply");
                return reply.result.map_err(anyhow::Error::msg);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn monitor_measurement_channels_keep_the_interactive_shell_ready() -> Result<()> {
    let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let fixture = Fixture::start(Mode::Monitor(counter.clone())).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    for _ in 0..2 {
        let Output::Table(sample) =
            request(&mut terminal, Operation::Query(Section::Monitor)).await?
        else {
            anyhow::bail!("Monitor output missing");
        };
        assert_eq!(sample.section, Section::Monitor);
        assert_eq!(sample.detail.split("__SEP__").count(), 5);
    }
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    terminal.write("monitor-shell-live\r")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = Vec::new();
        loop {
            while let Some(bytes) = terminal.try_read() {
                output.extend(bytes);
            }
            if String::from_utf8_lossy(&output).contains("monitor-shell-live") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    assert_eq!(terminal.status(), Status::Ready);
    Ok(())
}
#[tokio::test]
async fn management_channels_share_authentication_without_stealing_shell_output() -> Result<()> {
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    let Output::Table(ports) = request(&mut terminal, Operation::Query(Section::Ports)).await?
    else {
        anyhow::bail!("Ports")
    };
    assert_eq!(ports.rows[0], vec!["tcp", "[::]", "22", "42", "sshd"]);
    assert_eq!(ports.rows[1][2], "53");
    let Output::Table(docker) = request(&mut terminal, Operation::Query(Section::Docker)).await?
    else {
        anyhow::bail!("Docker")
    };
    assert_eq!(docker.rows[0][2], "web");
    terminal.write("中文\r")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = Vec::new();
        loop {
            while let Some(bytes) = terminal.try_read() {
                output.extend(bytes);
            }
            if String::from_utf8_lossy(&output).contains("中文") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    Ok(())
}
#[tokio::test]
async fn native_sftp_lists_edits_uploads_downloads_renames_and_removes_without_shell() -> Result<()>
{
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SFTP").await?;
    request(&mut terminal, Operation::Mkdir("/data".into())).await?;
    request(
        &mut terminal,
        Operation::WriteText {
            path: "/data/中文.txt".into(),
            text: "first\n中文".into(),
        },
    )
    .await?;
    let Output::Directory { entries, .. } =
        request(&mut terminal, Operation::List("/data".into())).await?
    else {
        anyhow::bail!("Directory")
    };
    assert_eq!(entries[0].name, "中文.txt");
    let Output::Text { text, .. } =
        request(&mut terminal, Operation::ReadText("/data/中文.txt".into())).await?
    else {
        anyhow::bail!("Text")
    };
    assert_eq!(text, "first\n中文");
    let Output::EditCached { local, .. } = request(
        &mut terminal,
        Operation::PrepareEdit {
            remote: "/data/中文.txt".into(),
            launch: false,
        },
    )
    .await?
    else {
        anyhow::bail!("Edit cache missing")
    };
    std::fs::write(local, "edited by default application 中文")?;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if std::fs::read_to_string(fixture.root.path().join("data/中文.txt"))
                .ok()
                .as_deref()
                == Some("edited by default application 中文")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("input.bin");
    let bytes: Vec<_> = (0..350_000).map(|i| (i % 256) as u8).collect();
    std::fs::write(&input, &bytes)?;
    request(
        &mut terminal,
        Operation::Upload {
            local: input,
            remote: "/data/payload.bin".into(),
        },
    )
    .await?;
    request(
        &mut terminal,
        Operation::Rename {
            from: "/data/payload.bin".into(),
            to: "/data/renamed.bin".into(),
        },
    )
    .await?;
    let output = directory.path().join("output.bin");
    std::fs::write(&output, b"previous")?;
    request(
        &mut terminal,
        Operation::Download {
            remote: "/data/renamed.bin".into(),
            local: output.clone(),
        },
    )
    .await?;
    assert_eq!(std::fs::read(output)?, bytes);
    let nested = directory.path().join("folder/sub");
    std::fs::create_dir_all(&nested)?;
    std::fs::write(nested.join("中文.bin"), &bytes)?;
    request(
        &mut terminal,
        Operation::Upload {
            local: directory.path().join("folder"),
            remote: "/data/folder".into(),
        },
    )
    .await?;
    request(
        &mut terminal,
        Operation::Download {
            remote: "/data/folder".into(),
            local: directory.path().join("download"),
        },
    )
    .await?;
    assert_eq!(
        std::fs::read(directory.path().join("download/sub/中文.bin"))?,
        bytes
    );
    let selection = tomato_services::files::export::Request::selection(vec![
        tomato_services::files::export::Item::new(
            "/data/renamed.bin".into(),
            Some(bytes.len() as u64),
            false,
        ),
        tomato_services::files::export::Item::new("/data/folder".into(), None, true),
    ])?;
    let copying = request(&mut terminal, Operation::ExportFile(selection.clone()));
    deferred_export::assert_after_drop(selection, copying, &bytes, "/data/folder/sub/中文.bin")
        .await?;
    request(
        &mut terminal,
        Operation::Remove {
            path: "/data".into(),
            directory: true,
        },
    )
    .await?;
    assert!(!fixture.root.path().join("data").exists());
    Ok(())
}
#[tokio::test]
async fn image_preview_reuses_authenticated_sftp_and_returns_full_or_thumbnail() -> Result<()> {
    use tomato_services::files::images::{Mode as ImageMode, Request};
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        3,
        6,
        image::Rgba([11, 22, 33, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)?;
    std::fs::write(fixture.root.path().join("中文.png"), bytes.into_inner())?;
    let mut terminal = fixture.terminal("SFTP").await?;
    for mode in [ImageMode::Thumbnail, ImageMode::Full] {
        let Output::Image { asset, .. } = request(
            &mut terminal,
            Operation::ReadImage(Request::new("/中文.png".into(), mode)),
        )
        .await?
        else {
            anyhow::bail!("image reply missing")
        };
        assert_eq!((asset.width, asset.height), (3, 6));
        assert_eq!(
            (asset.picture.width, asset.picture.height),
            if mode == ImageMode::Thumbnail {
                (24, 48)
            } else {
                (3, 6)
            }
        );
    }
    assert!(matches!(
        request(&mut terminal, Operation::List("/".into())).await?,
        Output::Directory { .. }
    ));
    Ok(())
}

fn java_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME")
        .map(|p| {
            PathBuf::from(p).join("bin").join(if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.into()
            })
        })
        .unwrap_or_else(|| PathBuf::from(name))
}
fn java_classes() -> Result<&'static PathBuf> {
    static CLASSES: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    CLASSES
        .get_or_init(|| -> Result<PathBuf, String> {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let target = root.join("../../target/test-java-zmodem");
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            fn collect(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
                for entry in std::fs::read_dir(path)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        collect(&path, files)?;
                    } else if path.extension().is_some_and(|s| s == "java") {
                        files.push(path);
                    }
                }
                Ok(())
            }
            let mut sources = Vec::new();
            collect(&root.join("tests/fixtures/legacy-zmodem"), &mut sources)
                .map_err(|e| e.to_string())?;
            sources.push(root.join("tests/fixtures/LegacyZmodemPeer.java"));
            let output = std::process::Command::new(java_tool("javac"))
                .args(["--release", "17", "-encoding", "UTF-8", "-d"])
                .arg(&target)
                .args(sources)
                .output()
                .map_err(|e| {
                    format!("JDK required for original Java ZMODEM interoperability tests: {e}")
                })?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            Ok(target)
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
}
async fn transfer(terminal: &RemoteTerminal, upload: bool, choice: Choice) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match terminal.transfer_state() {
                TransferState::ChooseUpload if upload => break,
                TransferState::ChooseDirectory if !upload => break,
                TransferState::Failed(e) => anyhow::bail!(e),
                _ => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    terminal.transfer_choice(choice)?;
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            match terminal.transfer_state() {
                TransferState::Completed => return Ok::<_, anyhow::Error>(()),
                TransferState::Failed(e) => anyhow::bail!(e),
                _ => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
    })
    .await?
}
#[tokio::test]
async fn rz_upload_matches_original_java_receiver_for_binary_unicode_and_empty_files() -> Result<()>
{
    let source = tempfile::tempdir()?;
    let destination = tempfile::tempdir()?;
    let data: Vec<_> = (0..270_000).map(|i| (i % 256) as u8).collect();
    let path = source.path().join("上传-中文.bin");
    std::fs::write(&path, &data)?;
    let empty = source.path().join("empty.txt");
    std::fs::write(&empty, b"")?;
    std::fs::write(destination.path().join("上传-中文.bin"), &data[..4096])?;
    // The original Java receiver skips an absent zero-byte destination.
    // Pre-create it to verify the skip path; native receive tests create it from scratch.
    std::fs::write(destination.path().join("empty.txt"), b"")?;
    let fixture = Fixture::start(Mode::JavaReceive(destination.path().into())).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    transfer(&terminal, true, Choice::Upload(vec![path, empty])).await?;
    assert_eq!(
        std::fs::read(destination.path().join("上传-中文.bin"))?,
        data
    );
    assert_eq!(
        std::fs::metadata(destination.path().join("empty.txt"))?.len(),
        0
    );
    let text = restored(&mut terminal).await?;
    assert_zmodem_notices(
        &text,
        &[
            "检测到rz上传请求，请选择要上传的文件...",
            "正在上传文件...",
            "上传完成",
        ],
    );
    Ok(())
}
#[tokio::test]
async fn sz_download_matches_original_java_sender_and_restores_terminal() -> Result<()> {
    let source = tempfile::tempdir()?;
    let destination = tempfile::tempdir()?;
    let data: Vec<_> = (0..330_000).map(|i| ((i * 31) % 256) as u8).collect();
    let path = source.path().join("download.bin");
    std::fs::write(&path, &data)?;
    let empty = source.path().join("empty.txt");
    std::fs::write(&empty, b"")?;
    let fixture = Fixture::start(Mode::JavaSend(vec![path, empty])).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    transfer(
        &terminal,
        false,
        Choice::Download(destination.path().into()),
    )
    .await?;
    assert_eq!(
        std::fs::read(destination.path().join("download.bin"))?,
        data
    );
    assert_eq!(
        std::fs::metadata(destination.path().join("empty.txt"))?.len(),
        0
    );
    let text = restored(&mut terminal).await?;
    assert_zmodem_notices(
        &text,
        &[
            "检测到sz下载请求，请选择保存目录...",
            &format!("正在下载文件到: {}", destination.path().display()),
            "下载完成",
        ],
    );
    Ok(())
}

#[path = "support/sftp_fs.rs"]
mod filesystem;
use filesystem::Fs;

#[tokio::test]
async fn referenced_native_jump_authenticates_both_hosts_and_multiplexes_sftp() -> Result<()> {
    let target = Fixture::start(Mode::Shell).await?;
    let jump = Fixture::start(Mode::Shell).await?;
    let make = |port: u16| {
        let mut c = Connection::new("SSH");
        c.set("port", port);
        c.set("username", "fixture");
        c.set("password", "fixture-only");
        c
    };
    let gateway = make(jump.port);
    let mut destination = make(target.port);
    destination.set("useSshTunnel", true);
    destination.set("sshTunnelHostId", gateway.id.clone());
    tomato_core::Store {
        version: 1,
        connections: vec![gateway, destination.clone()],
        ..Default::default()
    }
    .save(&jump.root.path().join("connections.json"))?;
    let mut terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        destination,
        jump.root.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(15), async {
        while terminal.status() != Status::Ready {
            if let Status::Failed(e) = terminal.status() {
                anyhow::bail!(e);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    request(
        &mut terminal,
        Operation::WriteText {
            path: "/through-jump.txt".into(),
            text: "nested authentication works".into(),
        },
    )
    .await?;
    assert_eq!(
        std::fs::read_to_string(target.root.path().join("through-jump.txt"))?,
        "nested authentication works"
    );
    assert!(!jump.root.path().join("through-jump.txt").exists());
    Ok(())
}

struct CaptureChild(Child);
#[tokio::test]
async fn shell_receives_tab_and_readline_control_bytes_without_local_rewriting() -> Result<()> {
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    let expected = b"ec\t\x08\x01\x05\x03";
    terminal.write(std::str::from_utf8(expected)?)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut wire = Vec::new();
        loop {
            while let Some(bytes) = terminal.try_read() {
                wire.extend(bytes);
            }
            if wire.windows(expected.len()).any(|bytes| bytes == expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}
impl Drop for CaptureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn captured(mut command: std::process::Command) -> Result<()> {
    let mut child = CaptureChild(command.spawn()?);
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Some(status) = child.0.try_wait()? {
                ensure!(status.success(), "Capture process failed: {status}");
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    Ok(())
}
#[tokio::test]
#[ignore = "requires TOMATO_SSH_CAPTURE_EXE and TOMATO_SSH_CAPTURE_PATH; optional original Java UI oracle"]
async fn capture_native_and_original_ssh_tools() -> Result<()> {
    let fixture_mode = std::env::var("TOMATO_SSH_TAB_FIXTURE").unwrap_or_default();
    let storage_peer = matches!(
        fixture_mode.as_str(),
        "storage" | "storage_error" | "storage_form"
    )
    .then(|| {
        Arc::new(std::sync::Mutex::new(storage_panel::Peer {
            reject: fixture_mode == "storage_error",
            ..Default::default()
        }))
    });
    let docker_peer = matches!(
        std::env::var("TOMATO_SSH_TAB_FIXTURE").as_deref(),
        Ok("docker" | "docker_tree")
    )
    .then(|| Arc::new(std::sync::Mutex::new(docker::Peer::default())));
    let monitor_peer =
        (fixture_mode == "monitor").then(|| Arc::new(std::sync::atomic::AtomicU64::new(0)));
    let fixture = Fixture::start(
        monitor_peer
            .as_ref()
            .map(|peer| Mode::Monitor(peer.clone()))
            .or_else(|| {
                docker_peer
                    .as_ref()
                    .map(|peer| Mode::Docker(peer.clone()))
                    .or_else(|| {
                        storage_peer
                            .as_ref()
                            .map(|peer| Mode::Storage(peer.clone()))
                    })
            })
            .unwrap_or(Mode::Shell),
    )
    .await?;
    std::fs::create_dir(fixture.root.path().join("logs"))?;
    std::fs::write(fixture.root.path().join("配置.json"), "{\"fixture\":true}")?;
    let mut c = Connection::new("SSH");
    c.name = "SSH 本地验收".into();
    c.set("port", fixture.port);
    c.set("username", "fixture");
    c.set("password", "fixture-only");
    let profile = tempfile::tempdir()?;
    tomato_core::Store {
        version: 1,
        connections: vec![c],
        ..Default::default()
    }
    .save(&profile.path().join("connections.json"))?;
    if std::env::var_os("TOMATO_TEST_TERMINAL_LAYOUT").is_some() {
        std::fs::write(
            profile.path().join("global.json"),
            br#"{"sshTerminalFontName":"Consolas","sshTerminalFontSize":18}"#,
        )?;
    }
    let configuration_before = std::fs::read(profile.path().join("connections.json"))?;
    let path =
        PathBuf::from(std::env::var_os("TOMATO_SSH_CAPTURE_PATH").context("Missing capture path")?);
    ensure!(!path.exists(), "Use a fresh capture path");
    let mut command = std::process::Command::new(
        std::env::var_os("TOMATO_SSH_CAPTURE_EXE").context("Missing capture executable")?,
    );
    if std::env::var_os("TOMATO_SSH_CAPTURE_HARNESS").is_some() {
        let entry = if std::env::var("TOMATO_SSH_CAPTURE_ENTRY").as_deref() == Ok("offscreen") {
            "capture_offscreen"
        } else {
            "capture_desktop"
        };
        command.args([entry, "--exact", "--ignored", "--nocapture"]);
    }
    command
        .env("TOMATO_DATA_DIR", profile.path())
        .env("TOMATO_UI_CAPTURE", &path)
        .env("TOMATO_UI_PREVIEW", "ssh");
    let check_blink = std::env::var_os("TOMATO_TEST_ASSERT_CURSOR_BLINK").is_some();
    if check_blink {
        command.env("TOMATO_UI_CAPTURE_FRAMES", "8");
    }
    captured(command).await?;
    ensure!(
        configuration_before == std::fs::read(profile.path().join("connections.json"))?,
        "Owned encrypted configuration changed during native control capture"
    );
    let image = image::open(&path)?.into_rgb8();
    if let Ok(mode) = std::env::var("TOMATO_SSH_TAB_FIXTURE") {
        let proof: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("ssh-tab.json"))?)?;
        ensure!(
            proof["mode"] == mode && proof["sessionReady"] == true && proof["osInputUsed"] == false,
            "SSH menu/window fixture did not complete: {proof}"
        );
        if let Some(peer) = &monitor_peer {
            ensure!(
                peer.load(std::sync::atomic::Ordering::SeqCst) >= 3,
                "Monitor did not poll actual SSH exec channels"
            );
            ensure!(
                proof["monitor"]["samples"].as_u64().is_some_and(|n| n >= 3)
                    && proof["monitor"]["rawRates"]
                        .as_array()
                        .is_some_and(|v| v.len() == 6)
                    && proof["monitor"]["diskReadBytes"].as_u64()
                        == proof["monitor"]["samples"]
                            .as_u64()
                            .map(|n| (n - 1) * 100 * 512)
                    && proof["monitor"]["diskWrittenBytes"].as_u64()
                        == proof["monitor"]["samples"]
                            .as_u64()
                            .map(|n| (n - 1) * 200 * 512),
                "Monitor did not consume complete samples: {proof}"
            );
        }
        if mode == "copy" {
            ensure!(
                proof["sessions"] == 2 && proof["readySessions"] == 2,
                "Fresh independent SSH session missing: {proof}"
            );
        }
        if let Some(peer) = &storage_peer {
            let p = peer.lock().unwrap();
            ensure!(
                proof["workflowComplete"] == true,
                "Storage controls did not complete"
            );
            ensure!(
                proof["unexpectedFilesPane"] == false,
                "Storage workflow created an unrelated SFTP pane"
            );
            match mode.as_str() {
                "storage" => ensure!(
                    p.initialized
                        && p.joined
                        && p.extended
                        && p.mounted
                        && p.attempts == 4
                        && p.queries >= 5,
                    "Storage mutations/readback missing"
                ),
                "storage_error" => ensure!(
                    p.attempts == 1 && !p.extended && proof["errorVisible"] == true,
                    "Storage failure was not shown without changing state"
                ),
                _ => ensure!(
                    p.attempts == 0 && proof["filesystemPopupVisible"] == true,
                    "Opening a mount form must not execute commands"
                ),
            }
            std::fs::write(
                path.with_extension("storage-server.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"passed":true,"realSsh":true,"actualBlockDevicesTouched":false,"mode":mode,"attempts":p.attempts,"queries":p.queries,"initialized":p.initialized,"joined":p.joined,"extended":p.extended,"mounted":p.mounted,"commands":p.commands}),
                )?,
            )?;
        }
        if mode == "docker_tree" {
            ensure!(
                proof["phase"] == 11
                    && proof["stableTabs"] == 1
                    && proof["treeRunning"] == true
                    && proof["sawStopped"] == true
                    && proof["jobReleased"] == true
                    && proof["credentialPrompts"] == 1,
                "Container tree did not complete without extra tabs: {proof}"
            );
            let peer = docker_peer
                .as_ref()
                .context("Missing Docker tree peer")?
                .lock()
                .unwrap();
            ensure!(
                peer.shell_input.is_empty(),
                "Credential input reached the background terminal"
            );
            let mutations = peer
                .commands
                .iter()
                .filter(|command| {
                    command.contains("docker stop ") || command.contains("docker start ")
                })
                .count();
            let listings = peer
                .commands
                .iter()
                .filter(|command| command.contains("docker ps "))
                .count();
            ensure!(
                peer.running && mutations == 2 && listings >= 3,
                "Container tree state did not match independent SSH peer"
            );
            std::fs::write(
                path.with_extension("docker-server.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"passed":true,"realSsh":true,"actualDockerDaemon":false,"running":peer.running,"mutations":mutations,"listings":listings,"commands":peer.commands}),
                )?,
            )?;
        }
        if mode == "docker" {
            ensure!(
                proof["phase"] == 15
                    && proof["memoryBytes"] == 2_u64 << 30
                    && proof["policy"] == "always"
                    && proof["busy"] == false,
                "Container controls did not save and refresh: {proof}"
            );
            let peer = docker_peer
                .as_ref()
                .context("Missing independent Docker SSH peer")?
                .lock()
                .unwrap();
            let updates = peer
                .commands
                .iter()
                .filter(|cmd| cmd.contains("docker update "))
                .count();
            let inspections = peer
                .commands
                .iter()
                .filter(|cmd| cmd.contains("docker inspect "))
                .count();
            ensure!(
                peer.memory == 2_u64 << 30
                    && peer.swap == (2_i64 << 30) + (512 << 20)
                    && peer.policy == "always"
                    && updates == 2
                    && inspections >= 3,
                "Native controls failed independent server validation"
            );
            std::fs::write(
                path.with_extension("docker-server.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"passed":true,"realSsh":true,"actualDockerDaemon":false,"memoryBytes":peer.memory,"swapBytes":peer.swap,"policy":peer.policy,"updates":updates,"inspections":inspections,"commands":peer.commands}),
                )?,
            )?;
        }
        ensure!(
            if mode == "session" {
                image.width() >= 380 && image.height() >= 150
            } else {
                image.width() >= 500 && image.height() >= 600
            },
            "SSH context window screenshot missing"
        );
        return Ok(());
    }
    ensure!(
        image.width() >= 500
            && image.height() >= 600
            && image.pixels().filter(|p| p.0 == [30, 30, 30]).count() > 10_000,
        "SSH terminal did not render"
    );
    if check_blink {
        use image::GenericImageView as _;
        ensure!(
            image.width() >= 1000,
            "Blink regression requires the full-width native fixture"
        );
        let mut origin = None;
        'rows: for y in 0..image.height() {
            let mut consecutive = 0;
            for x in 0..image.width() {
                if image.get_pixel(x, y).0 == [30, 30, 30] {
                    consecutive += 1;
                    if consecutive == 200 {
                        origin = Some((x + 1 - 200, y));
                        break 'rows;
                    }
                } else {
                    consecutive = 0;
                }
            }
        }
        let (left, top) = origin.context("Missing terminal viewport")?;
        let mut counts = Vec::new();
        for index in 0..8 {
            let frame = if index == 0 {
                path.clone()
            } else {
                path.with_file_name(format!(
                    "{}-{index:02}.png",
                    path.file_stem().unwrap_or_default().to_string_lossy()
                ))
            };
            let frame = image::open(frame)?.into_rgb8();
            let pixels = image::imageops::crop_imm(
                &frame,
                left,
                top,
                350.min(frame.width() - left),
                22.min(frame.height() - top),
            );
            counts.push(
                pixels
                    .pixels()
                    .filter(|(_, _, pixel)| pixel.0 == [229, 229, 229])
                    .count(),
            );
        }
        eprintln!("Idle cursor foreground pixel counts: {counts:?}");
        ensure!(
            counts.iter().max().unwrap() - counts.iter().min().unwrap() >= 64,
            "Idle native terminal cursor never changed between visible and hidden phases"
        );
    }
    if let (Some(java), Some(classpath)) = (
        std::env::var_os("TOMATO_LEGACY_JAVA"),
        std::env::var_os("TOMATO_LEGACY_SSH_CLASSPATH"),
    ) {
        let legacy = path.with_extension("legacy.png");
        ensure!(!legacy.exists(), "Use a fresh legacy capture path");
        let home = tempfile::tempdir()?;
        let mut command = std::process::Command::new(java);
        command
            .arg("-Xmx256m")
            .arg("-Dfile.encoding=UTF-8")
            .arg("-Dprism.order=sw")
            .arg(format!(
                "-Dtomato.capture.offscreen={}",
                std::env::var("TOMATO_SSH_CAPTURE_ENTRY").as_deref() == Ok("offscreen")
            ))
            .arg(format!("-Duser.home={}", home.path().display()))
            .arg("-cp")
            .arg(classpath)
            .arg("LegacySshCapture")
            .arg(fixture.port.to_string())
            .arg(&legacy);
        captured(command).await?;
        ensure!(legacy.exists(), "Original UI capture missing");
    }
    Ok(())
}

fn assert_zmodem_notices(mut text: &str, messages: &[&str]) {
    for message in messages {
        let notice = format!("\r\n[ZModem] {message}\r\n");
        let (_, remainder) = text
            .split_once(&notice)
            .unwrap_or_else(|| panic!("missing notice {message:?} in {text:?}"));
        text = remainder;
    }
}

async fn restored(terminal: &mut RemoteTerminal) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut text = Vec::new();
        loop {
            while let Some(bytes) = terminal.try_read() {
                text.extend(bytes);
            }
            if String::from_utf8_lossy(&text).contains("fixture$ ") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        ensure!(
            !text.windows(2).any(|p| p == b"OO"),
            "ZMODEM finish bytes leaked to the terminal: {text:?}"
        );
        terminal.write("echo-after-transfer\r")?;
        let mut echo = Vec::new();
        loop {
            while let Some(bytes) = terminal.try_read() {
                echo.extend(bytes);
            }
            if String::from_utf8_lossy(&echo).contains("echo-after-transfer") {
                return Ok(String::from_utf8_lossy(&text).into_owned());
            }
            if let Status::Failed(error) = terminal.status() {
                anyhow::bail!(error);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn cancelling_remote_rz_returns_to_interactive_shell() -> Result<()> {
    let destination = tempfile::tempdir()?;
    let fixture = Fixture::start(Mode::JavaReceive(destination.path().into())).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if terminal.transfer_state() == TransferState::ChooseUpload {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    terminal.transfer_choice(Choice::Cancel)?;
    let text = restored(&mut terminal).await?;
    assert_zmodem_notices(
        &text,
        &[
            "检测到rz上传请求，请选择要上传的文件...",
            "未选择文件，取消上传",
        ],
    );
    assert_eq!(terminal.status(), Status::Ready);
    assert_eq!(std::fs::read_dir(destination.path())?.count(), 0);
    Ok(())
}

async fn native_sender(
    mut stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    files: Vec<PathBuf>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use zmodem2::{Action, Event, FileInfo, Position, Sender};
    let mut machine = Sender::new()?;
    let mut files = files.into_iter();
    let mut data = Vec::new();
    let mut next = true;
    let mut finished = false;
    let mut wire = [0; 4096];
    let mut pending = Vec::new();
    loop {
        match machine.poll() {
            Action::WriteWire(bytes) => {
                let n = bytes.len();
                stream.write_all(bytes).await?;
                machine.wire_written(n);
            }
            Action::ReadFile { offset, max_len } => {
                let start = offset.get() as usize;
                let end = (start + max_len).min(data.len());
                machine.submit_file(&data[start..end])?;
            }
            Action::Event(Event::FileCompleted) => next = true,
            Action::Event(Event::SessionCompleted) => finished = true,
            Action::Event(Event::Aborted) => anyhow::bail!("Native peer cancelled"),
            Action::Idle if finished => break,
            Action::Idle if next => {
                next = false;
                if let Some(path) = files.next() {
                    data = std::fs::read(&path)?;
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .context("fixture filename")?;
                    machine.start_file(FileInfo::new(
                        name.as_bytes(),
                        Some(Position::new(u32::try_from(data.len())?)),
                    ))?;
                } else {
                    machine.finish()?;
                }
            }
            Action::Idle => {
                if pending.is_empty() {
                    let n = tokio::time::timeout(Duration::from_secs(15), stream.read(&mut wire))
                        .await??;
                    ensure!(n > 0, "Native peer EOF");
                    pending.extend_from_slice(&wire[..n]);
                }
                let count = machine.submit_wire(&pending)?;
                ensure!(count > 0, "Native fixture state machine stalled");
                pending.drain(..count);
            }
            _ => {}
        }
    }
    stream.write_all(b"\r\nfixture$ ").await?;
    loop {
        let n = stream.read(&mut wire).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&wire[..n]).await?;
    }
    Ok(())
}
#[tokio::test]
async fn sz_crc32_download_preserves_utf8_names_binary_and_new_empty_files() -> Result<()> {
    let source = tempfile::tempdir()?;
    let destination = tempfile::tempdir()?;
    let bytes: Vec<_> = (0..160_000).map(|i| (i % 256) as u8).collect();
    let file = source.path().join("下载-中文.bin");
    std::fs::write(&file, &bytes)?;
    let empty = source.path().join("空文件.txt");
    std::fs::write(&empty, b"")?;
    let fixture = Fixture::start(Mode::NativeSend(vec![file, empty])).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    transfer(
        &terminal,
        false,
        Choice::Download(destination.path().into()),
    )
    .await?;
    assert_eq!(
        std::fs::read(destination.path().join("下载-中文.bin"))?,
        bytes
    );
    assert_eq!(
        std::fs::metadata(destination.path().join("空文件.txt"))?.len(),
        0
    );
    restored(&mut terminal).await?;
    Ok(())
}

#[tokio::test]
async fn large_sftp_directory_fails_at_bound_and_keeps_session_usable() -> Result<()> {
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SFTP").await?;
    let error = request(&mut terminal, Operation::List("/too-many".into()))
        .await
        .expect_err("Directory must stop at configured bound");
    assert!(
        error.to_string().contains("directory entry limit"),
        "{error:#}"
    );
    request(
        &mut terminal,
        Operation::WriteText {
            path: "/after-limit.txt".into(),
            text: "still connected".into(),
        },
    )
    .await?;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("after-limit.txt"))?,
        "still connected"
    );
    assert_eq!(terminal.status(), Status::Ready);
    Ok(())
}

#[tokio::test]
async fn scoped_tcp_tunnel_reuses_original_jump_auth_and_closes_sockets() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tomato_services::terminal::ssh::tunnel::Tunnel;
    let jump = Fixture::start(Mode::Shell).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let echo = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                result=listener.accept()=>{let Ok((socket,_))=result else{break;};clients.spawn(async move {let (mut r,mut w)=socket.into_split();let _=tokio::io::copy(&mut r,&mut w).await;});},
                _=clients.join_next(),if !clients.is_empty()=>{},
            }
        }
    });
    let mut gateway = Connection::new("SSH");
    gateway.set("host", "127.0.0.1");
    gateway.set("port", jump.port);
    gateway.set("username", "fixture");
    gateway.set("password", "fixture-only");
    let mut target = Connection::new("REDIS");
    target.set("host", "127.0.0.1");
    target.set("port", port);
    let tunnel = Tunnel::open_via(&target, &gateway, jump.root.path().join("known_hosts")).await?;
    let address = tunnel.address;
    let data = vec![173u8; 256 * 1024];
    for _ in 0..3 {
        let mut socket = tokio::net::TcpStream::connect(address).await?;
        let (mut read, mut write) = socket.split();
        let send = async {
            write.write_all(&data).await?;
            write.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        };
        let receive = async {
            let mut result = Vec::new();
            read.read_to_end(&mut result).await?;
            Ok::<_, anyhow::Error>(result)
        };
        let (_, result) = tokio::try_join!(send, receive)?;
        assert_eq!(result, data);
    }
    let mut live = tokio::net::TcpStream::connect(address).await?;
    live.write_all(b"alive").await?;
    let mut response = [0u8; 5];
    live.read_exact(&mut response).await?;
    assert_eq!(&response, b"alive");
    drop(tunnel);
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut byte = [0];
        assert!(matches!(live.read(&mut byte).await, Ok(0) | Err(_)));
    })
    .await?;
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    assert_eq!(target.text("host"), "127.0.0.1");
    assert_eq!(target.number("port"), u64::from(port));
    echo.abort();
    let _ = echo.await;
    Ok(())
}

#[tokio::test]
async fn inline_legacy_jump_authenticates_target_ssh_without_a_referenced_host() -> Result<()> {
    let target = Fixture::start(Mode::Shell).await?;
    let jump = Fixture::start(Mode::Shell).await?;
    let mut connection = Connection::new("SSH");
    connection.set("port", target.port);
    connection.set("username", "fixture");
    connection.set("password", "fixture-only");
    connection.set("useSshTunnel", true);
    connection.set("sshTunnelHost", "127.0.0.1");
    connection.set("sshTunnelPort", jump.port);
    connection.set("sshTunnelUsername", "fixture");
    connection.set("sshTunnelPassword", "fixture-only");
    connection.set("sshTunnelUsePassword", true);
    let mut terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        connection,
        jump.root.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match terminal.status() {
                Status::Ready => return Ok::<_, anyhow::Error>(()),
                Status::Failed(error) => anyhow::bail!(error),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await??;
    request(
        &mut terminal,
        Operation::WriteText {
            path: "/legacy-through-jump.txt".into(),
            text: "inline compatibility".into(),
        },
    )
    .await?;
    assert_eq!(
        std::fs::read_to_string(target.root.path().join("legacy-through-jump.txt"))?,
        "inline compatibility"
    );
    assert!(!jump.root.path().join("legacy-through-jump.txt").exists());
    Ok(())
}
#[path = "support/deferred_export.rs"]
mod deferred_export;
