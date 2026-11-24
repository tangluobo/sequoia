use super::RunItem;
use anyhow::{Context, Result, ensure};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;
use tomato_core::development::{Config, Project};
pub const LOG_LIMIT: usize = tomato_core::development::log::LIMIT;
#[derive(Clone, Debug)]
pub struct Plan {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub directory: PathBuf,
    pub env: BTreeMap<String, String>,
}
impl Plan {
    pub fn new(executable: impl Into<PathBuf>, directory: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            directory: directory.into(),
            args: vec![],
            env: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Log {
    pub root: PathBuf,
    pub text: String,
    pub offset: u64,
    pub reset: bool,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub dependencies: bool,
}
#[derive(Default)]
pub struct Buffer {
    text: String,
    base: u64,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub dependencies: bool,
}
impl Buffer {
    pub fn append(&mut self, text: &str) {
        self.base += tomato_core::development::log::append(&mut self.text, text) as u64;
    }
}
pub type Shared = Arc<Mutex<Buffer>>;
pub fn append(shared: &Shared, text: &str) {
    if let Ok(mut state) = shared.lock() {
        state.append(text)
    }
}
struct Job {
    task: tokio::task::JoinHandle<()>,
    cancel: CancellationToken,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
#[derive(Default)]
pub struct Processes {
    jobs: HashMap<PathBuf, Job>,
    logs: HashMap<PathBuf, Shared>,
    builds: super::build::Cache,
}
impl Processes {
    pub async fn launch(
        &mut self,
        config: Config,
        project: Project,
        item: RunItem,
        restart: bool,
    ) -> Result<()> {
        self.start(config, project, Some(item), restart).await
    }
    pub async fn dependencies(&mut self, config: Config, project: Project) -> Result<()> {
        self.start(config, project, None, true).await
    }
    async fn start(
        &mut self,
        config: Config,
        project: Project,
        item: Option<RunItem>,
        restart: bool,
    ) -> Result<()> {
        let root = PathBuf::from(&project.path);
        ensure!(root.is_dir(), "项目目录不存在");
        self.jobs.retain(|_, job| !job.task.is_finished());
        if !self.logs.contains_key(&root)
            && self.logs.len() >= 32
            && let Some(old) = self
                .logs
                .keys()
                .find(|key| !self.jobs.contains_key(*key))
                .cloned()
        {
            self.logs.remove(&old);
        }
        if self
            .jobs
            .get(&root)
            .is_some_and(|job| !job.task.is_finished())
        {
            ensure!(restart, "当前项目已有程序正在运行，请先停止");
            self.stop(&root).await;
        }
        ensure!(
            self.jobs.len() < 32 || self.jobs.contains_key(&root),
            "同时管理的项目超过 32 个"
        );
        let base = self
            .logs
            .get(&root)
            .and_then(|log| log.lock().ok().map(|s| s.base + s.text.len() as u64 + 1))
            .unwrap_or(0);
        let log = Arc::new(Mutex::new(Buffer {
            base,
            running: true,
            dependencies: item.is_none(),
            ..Default::default()
        }));
        self.logs.insert(root.clone(), log.clone());
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let cache = self.builds.clone();
        let task = tokio::spawn(async move {
            let result = super::build::execute(&config, &project, item, &log, &token, &cache).await;
            let code = match result {
                Ok(code) => code,
                Err(e) => {
                    append(&log, &format!("\n运行失败：{e:#}\n"));
                    -1
                }
            };
            append(&log, &format!("\n进程结束，退出码：{code}\n"));
            if let Ok(mut s) = log.lock() {
                s.running = false;
                s.exit_code = Some(code);
            }
        });
        self.jobs.insert(root, Job { task, cancel });
        Ok(())
    }
    pub async fn stop(&mut self, root: &Path) {
        if let Some(mut job) = self.jobs.remove(root) {
            job.cancel.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(3), &mut job.task).await;
        }
        if let Some(log) = self.logs.get(root)
            && let Ok(mut s) = log.lock()
        {
            s.running = false;
            s.append("\n程序已停止\n");
        }
    }
    pub async fn shutdown(&mut self) {
        // Cancel all project trees immediately, then join concurrently under
        // one deadline instead of allowing three seconds for each of 32 jobs.
        let jobs = std::mem::take(&mut self.jobs);
        for job in jobs.values() {
            job.cancel.cancel();
        }
        let mut waiting = futures_util::stream::FuturesUnordered::new();
        for (_, mut job) in jobs {
            waiting.push(async move {
                let _ = (&mut job.task).await;
            });
        }
        use futures_util::StreamExt as _;
        let _ = tokio::time::timeout(Duration::from_secs(3), async {
            while waiting.next().await.is_some() {}
        })
        .await;
        self.logs.clear();
        self.builds.clear();
    }
    pub fn clear(&mut self, root: &Path) {
        if let Some(log) = self.logs.get(root)
            && let Ok(mut s) = log.lock()
        {
            s.base += s.text.len() as u64 + 1;
            s.text.clear();
        }
    }
    pub fn poll(&self, root: &Path, after: u64) -> Log {
        let default = Arc::new(Mutex::new(Buffer::default()));
        let shared = self.logs.get(root).unwrap_or(&default);
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        let end = s.base + s.text.len() as u64;
        let mut reset = after < s.base || after > end;
        let offset = if reset {
            0
        } else {
            after.saturating_sub(s.base) as usize
        };
        let offset = if s.text.is_char_boundary(offset) {
            offset
        } else {
            reset = true;
            0
        };
        let result = Log {
            root: root.into(),
            text: s.text[offset..].into(),
            offset: end,
            reset,
            running: s.running,
            exit_code: s.exit_code,
            dependencies: s.dependencies,
        };
        if !reset && offset > 0 {
            s.text.drain(..offset);
            s.base += offset as u64;
            if s.text.capacity() > 65536 && s.text.len() < 16384 {
                s.text.shrink_to(32768);
            }
        }
        result
    }
}
fn command(plan: &Plan) -> Result<Command> {
    let batch = cfg!(windows)
        && plan
            .executable
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut command = if batch {
        let program = plan.executable.to_string_lossy();
        ensure!(
            !program.contains(['"', '%', '\r', '\n']),
            "批处理路径含不支持的特殊字符"
        );
        let mut script = format!("\"\"{program}\"");
        for arg in &plan.args {
            ensure!(
                !arg.contains(['"', '%', '\r', '\n']),
                "批处理参数含不支持的特殊字符"
            );
            script.push_str(&format!(" \"{arg}\""));
        }
        script.push('"');
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/v:off", "/s", "/c"]);
        #[cfg(windows)]
        c.raw_arg(script);
        c
    } else {
        let mut c = Command::new(&plan.executable);
        c.args(&plan.args);
        c
    };
    command
        .current_dir(&plan.directory)
        .envs(&plan.env)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(unix)]
    command.process_group(0);
    Ok(command)
}
async fn read(
    mut pipe: impl tokio::io::AsyncRead + Unpin,
    log: &Shared,
    capture: bool,
) -> Result<Vec<u8>> {
    let mut retained = VecDeque::new();
    let mut bytes = [0u8; 4096];
    let mut tail = Vec::new();
    let mut ansi = tomato_core::development::AnsiText::default();
    let mut previous_cr = false;
    // Java BufferedReader.readLine normalizes CR, LF and CRLF. Keep that state
    // across pipe reads (including ANSI fragments), without retaining lines.
    let mut emit = |text: &str| {
        let clean = ansi.push(text);
        let mut normalized = String::with_capacity(clean.len());
        for c in clean.chars() {
            if c == '\r' {
                normalized.push('\n');
            } else if c != '\n' || !previous_cr {
                normalized.push(c);
            }
            previous_cr = c == '\r';
        }
        append(log, &normalized);
    };
    loop {
        let n = pipe.read(&mut bytes).await?;
        if n == 0 {
            break;
        }
        if capture {
            let excess = (retained.len() + n).saturating_sub(LOG_LIMIT);
            retained.drain(..excess);
            let required = retained.len() + n;
            if retained.capacity() < required {
                retained
                    .reserve_exact(required.next_power_of_two().min(LOG_LIMIT) - retained.len());
            }
            retained.extend(&bytes[..n]);
        }
        tail.extend_from_slice(&bytes[..n]);
        let valid = match std::str::from_utf8(&tail) {
            Ok(s) => {
                emit(s);
                tail.clear();
                continue;
            }
            Err(e) => e.valid_up_to(),
        };
        if valid > 0 {
            emit(&String::from_utf8_lossy(&tail[..valid]));
            tail.drain(..valid);
        }
        if tail.len() > 4 {
            emit(&String::from_utf8_lossy(&tail));
            tail.clear();
        }
    }
    if !tail.is_empty() {
        emit(&String::from_utf8_lossy(&tail));
    }
    Ok(retained.into())
}
pub async fn run(
    plan: Plan,
    log: &Shared,
    cancel: &CancellationToken,
    timeout: Option<Duration>,
) -> Result<(i32, String)> {
    ensure!(!cancel.is_cancelled(), "运行已取消");
    append(
        log,
        &format!(
            "\n命令：{} {}\n",
            plan.executable.display(),
            plan.args.join(" ")
        ),
    );
    let mut wrapped = CommandWrap::from(command(&plan)?);
    wrapped.wrap(KillOnDrop);
    #[cfg(windows)]
    wrapped
        .wrap(process_wrap::tokio::JobObject)
        .wrap(process_wrap::tokio::CreationFlags(
            windows::Win32::System::Threading::CREATE_NO_WINDOW,
        ));
    #[cfg(unix)]
    wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
    let mut child = wrapped
        .spawn()
        .with_context(|| format!("无法启动 {}", plan.executable.display()))?;
    let stdout = child.stdout().take().context("缺少标准输出")?;
    let stderr = child.stderr().take().context("缺少错误输出")?;
    let io = async {
        let (exit, out, err) = tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            read(stdout, log, timeout.is_some()),
            read(stderr, log, timeout.is_some())
        )?;
        let mut output = String::from_utf8_lossy(&out).into_owned();
        output.push_str(&String::from_utf8_lossy(&err));
        Ok::<_, anyhow::Error>((exit.code().unwrap_or(-1), output))
    };
    tokio::select! {result=io=>result,_=cancel.cancelled()=>anyhow::bail!("运行已取消"),_=tokio::time::sleep(timeout.unwrap_or(Duration::from_secs(365*24*3600)))=>anyhow::bail!("命令执行超时")}
}
pub async fn capture(plan: Plan, timeout: Duration) -> Result<String> {
    let log = Shared::default();
    let (code, text) = run(plan, &log, &CancellationToken::new(), Some(timeout)).await?;
    ensure!(code == 0, "命令失败：{text}");
    Ok(text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn quoted_batch_paths_and_arguments_keep_metacharacters_literal() -> Result<()> {
        let dir = tempfile::Builder::new()
            .prefix("tomato & owned-")
            .tempdir()?;
        let script = dir.path().join("quoted & wrapper.cmd");
        std::fs::write(&script, "@echo off\r\n@echo %1\r\n@echo %2\r\n")?;
        let mut plan = Plan::new(script, dir.path());
        plan.args = vec![
            "a&echo injected>marker".into(),
            "spaces | < > & value".into(),
        ];
        let (code, output) = run(
            plan,
            &Shared::default(),
            &CancellationToken::new(),
            Some(Duration::from_secs(10)),
        )
        .await?;
        assert_eq!(code, 0, "{output}");
        assert!(output.contains("\"a&echo injected>marker\""), "{output}");
        assert!(output.contains("\"spaces | < > & value\""), "{output}");
        assert!(!dir.path().join("marker").exists());
        Ok(())
    }
    #[tokio::test]
    async fn bounded_capture_keeps_final_build_diagnostics_and_normalizes_split_lines() -> Result<()>
    {
        use tokio::io::AsyncWriteExt;
        // A capacity of one splits every UTF-8 character, CRLF and ANSI escape.
        let (mut writer, reader) = tokio::io::duplex(1);
        let log = Shared::default();
        let task = tokio::spawn(async move {
            writer
                .write_all("中文\r\u{1b}[31m\nnext\rfinal\n".as_bytes())
                .await?;
            writer.shutdown().await
        });
        let raw = read(reader, &log, true).await?;
        task.await??;
        assert_eq!(log.lock().unwrap().text, "中文\nnext\nfinal\n");
        assert_eq!(raw, "中文\r\u{1b}[31m\nnext\rfinal\n".as_bytes());
        let (mut writer, reader) = tokio::io::duplex(16384);
        let log = Shared::default();
        let task = tokio::spawn(async move {
            let chunk = vec![b'x'; 16384];
            for _ in 0..140 {
                writer.write_all(&chunk).await?;
            }
            writer
                .write_all(b"\n[ERROR] FINAL DIAGNOSTIC\nTOMATO_CLASSPATH=owned.jar\n")
                .await?;
            writer.shutdown().await
        });
        let raw = read(reader, &log, true).await?;
        task.await??;
        assert_eq!(raw.len(), LOG_LIMIT);
        assert!(raw.ends_with(b"TOMATO_CLASSPATH=owned.jar\n"));
        let text = &log.lock().unwrap().text;
        assert!(text.len() <= LOG_LIMIT);
        assert!(text.contains("[ERROR] FINAL DIAGNOSTIC"));
        Ok(())
    }
    #[tokio::test]
    async fn live_output_is_streamed_without_a_second_retained_history() -> Result<()> {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(4096);
        let log = Shared::default();
        let task = tokio::spawn(async move {
            for _ in 0..20_000 {
                writer
                    .write_all("\u{1b}[31m中文\u{1b}[0m\n".as_bytes())
                    .await?;
            }
            writer.shutdown().await
        });
        let captured = read(reader, &log, false).await?;
        task.await??;
        assert!(captured.is_empty());
        assert_eq!(captured.capacity(), 0);
        assert_eq!(log.lock().unwrap().text, "中文\n".repeat(20_000));
        Ok(())
    }
    #[test]
    fn unicode_logs_are_bounded_and_clear_invalidates_old_cursors() {
        let root = PathBuf::from("owned-fixture");
        let mut processes = Processes::default();
        let shared = Shared::default();
        shared.lock().unwrap().append(&"中文".repeat(500_000));
        processes.logs.insert(root.clone(), shared);
        let first = processes.poll(&root, 0);
        assert!(first.reset);
        assert!(first.text.len() <= LOG_LIMIT);
        assert!(first.text.chars().all(|c| c == '中' || c == '文'));
        assert!(processes.poll(&root, first.offset).text.is_empty());
        let held = processes.logs.get(&root).unwrap().lock().unwrap();
        assert!(held.text.is_empty());
        assert!(
            held.text.capacity() <= 65536,
            "acknowledged backend history must release its allocation"
        );
        drop(held);

        processes.clear(&root);
        let cleared = processes.poll(&root, first.offset);
        assert!(cleared.reset);
        assert!(cleared.text.is_empty());
    }
}
