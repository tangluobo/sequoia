//! Parsing is isolated from the desktop heap and can be cancelled even inside an ODBC driver.
use super::*;
use process_wrap::std::{ChildWrapper, CommandWrap};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};
#[derive(Serialize, Deserialize)]
pub enum WorkerRequest {
    Read(Box<Source>),
    OdbcCatalog(Odbc),
    OdbcSources(bool),
}
#[derive(Serialize, Deserialize)]
pub enum WorkerResponse {
    Catalog(Catalog),
}
struct Process(Box<dyn ChildWrapper>);
impl Process {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        // Poll only the root: JobObject::try_wait drains the completion port
        // without remembering ACTIVE_PROCESS_ZERO for its subsequent wait.
        self.0.inner_mut().try_wait()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // The completion port has not been polled. Wait exactly once after
        // terminating the owned job/group, including its native descendants.
        let _ = self.0.start_kill();
        let _ = self.0.wait();
    }
}
pub(super) fn run(
    request: WorkerRequest,
    directory: &Path,
    control: &Control,
) -> Result<WorkerResponse> {
    control.check()?;
    let exe = std::env::var_os("TOMATO_IMPORT_SOURCE_EXE")
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)?;
    let stdout = File::create_new(directory.join("stdout.log"))?;
    let stderr = File::create_new(directory.join("stderr.log"))?;
    let mut command = Command::new(exe);
    command
        .arg("--tomato-data-import-worker")
        .arg(directory)
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(stderr);
    let mut wrapped = CommandWrap::from(command);
    #[cfg(windows)]
    {
        wrapped.wrap(process_wrap::std::CreationFlags(
            windows::Win32::System::Threading::CREATE_NO_WINDOW,
        ));
        wrapped.wrap(process_wrap::std::JobObject);
    }
    #[cfg(unix)]
    wrapped.wrap(process_wrap::std::ProcessGroup::leader());
    let mut child = Process(wrapped.spawn().context("无法启动导入解析进程")?);
    let bytes = serde_json::to_vec(&request)?;
    ensure!(bytes.len() <= 4 * 1024 * 1024, "源设置超过 4 MiB");
    {
        let mut stdin = child.0.stdin().take().context("无法打开解析进程输入")?;
        stdin.write_all(&bytes)?;
    }
    let status = loop {
        control.check()?;
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    if !status.success() {
        let mut error = String::new();
        File::open(directory.join("stderr.log"))?
            .take(8192)
            .read_to_string(&mut error)?;
        anyhow::bail!(
            "{}",
            if error.trim().is_empty() {
                "源数据解析进程异常结束"
            } else {
                error.trim()
            }
        )
    }
    let mut bytes = Vec::new();
    File::open(directory.join("catalog.json"))?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "源元数据超过 16 MiB，请分批选择文件"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn import_worker_reaps_exited_or_cancelled_jobs_after_root_exit_was_polled() -> Result<()> {
        for completed in [true, false] {
            let mut command =
                Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
            command
                .args(["/d", "/c", if completed { "exit 0" } else { "more" }])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut wrapped = CommandWrap::from(command);
            wrapped.wrap(process_wrap::std::CreationFlags(
                windows::Win32::System::Threading::CREATE_NO_WINDOW,
            ));
            wrapped.wrap(process_wrap::std::JobObject);
            let mut process = Process(wrapped.spawn()?);
            if completed {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while process.0.inner_mut().try_wait()?.is_none() {
                    ensure!(
                        std::time::Instant::now() < deadline,
                        "owned child did not exit"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                std::thread::sleep(Duration::from_millis(30));
                // Polling the production wrapper must preserve its job's
                // queued completion event for the single teardown wait.
                assert!(process.try_wait()?.is_some());
            }
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                drop(process);
                let _ = tx.send(());
            });
            rx.recv_timeout(Duration::from_secs(2))
                .context("owned job teardown blocked")?;
        }
        Ok(())
    }
}
/// Called before initializing graphics or loading any user configuration.
pub fn worker_entry() -> Option<i32> {
    let mut args = std::env::args_os();
    let _ = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--tomato-data-import-worker")) {
        return None;
    }
    let result = (|| -> Result<()> {
        let directory = PathBuf::from(args.next().context("缺少导入缓存目录")?);
        ensure!(args.next().is_none(), "导入解析参数无效");
        ensure!(
            directory.is_dir()
                && !directory.is_symlink()
                && directory
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("tomato-import-")),
            "导入缓存目录无效"
        );
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4 * 1024 * 1024, "源设置过大");
        let request: WorkerRequest = serde_json::from_slice(&bytes)?;
        let control = Control::default();
        let catalog = match request {
            WorkerRequest::Read(source) => materialize(&source, &directory, &control)?,
            WorkerRequest::OdbcCatalog(source) => odbc::catalog(&source, &directory)?,
            WorkerRequest::OdbcSources(bits) => odbc::sources(bits, &directory)?,
        };
        let bytes = serde_json::to_vec(&WorkerResponse::Catalog(catalog))?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "源元数据超过 16 MiB，请分批选择文件"
        );
        let mut output = File::create_new(directory.join("catalog.json"))?;
        output.write_all(&bytes)?;
        Ok(())
    })();
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            if e.downcast_ref::<odbc_api::Error>().is_some() {
                let _ = writeln!(
                    std::io::stderr(),
                    "ODBC 读取失败，请检查源查询、字段类型和驱动配置"
                );
            } else {
                let _ = writeln!(std::io::stderr(), "{e:#}");
            }
            1
        }
    })
}
