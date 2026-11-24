//! Transient local sudo adapter. Credentials never enter JSON, argv or process output.
use super::LIMIT;
use crate::backup::Control;
use anyhow::{Context, Result, ensure};
use process_wrap::std::CommandWrap;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct Password(Zeroizing<String>);
impl Password {
    pub fn edit(&mut self) -> &mut String {
        &mut self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn into_authorization(self, control: Control) -> Result<Authorization> {
        ensure!(
            self.0.len() <= 1024 && !self.0.contains(['\r', '\n', '\0']),
            "sudo 密码不能包含换行，且不能超过 1024 字节"
        );
        Ok(Authorization {
            password: Arc::new(self),
            control,
        })
    }
}
impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
#[derive(Clone, Debug)]
pub struct Authorization {
    password: Arc<Password>,
    pub(super) control: Control,
}
impl Authorization {
    pub(super) fn queued_bytes(&self) -> usize {
        self.password.0.capacity()
    }
}

pub(super) fn write(path: &Path, text: &str, authorization: &Authorization) -> Result<()> {
    write_with_timeout(path, text, authorization, Duration::from_secs(20))
}
pub(super) fn restore(path: &Path, text: &str, authorization: &Authorization) -> Result<()> {
    let recovery = Authorization {
        password: authorization.password.clone(),
        control: Control::default(),
    };
    write_with_timeout(path, text, &recovery, Duration::from_secs(3))
}
fn write_with_timeout(
    path: &Path,
    text: &str,
    authorization: &Authorization,
    timeout: Duration,
) -> Result<()> {
    ensure!(text.len() as u64 <= LIMIT, "Hosts 文件超过 8 MiB");
    let mut source = tempfile::Builder::new()
        .prefix("tomato-hosts-")
        .tempfile()?;
    source.write_all(text.as_bytes())?;
    source.as_file().sync_all()?;
    // An absolute trusted executable and separate arguments avoid PATH and shell expansion.
    let mut command = Command::new("/usr/bin/sudo");
    command
        .args(["-S", "-k", "-p", "", "--", "/bin/cp", "--"])
        .arg(source.path())
        .arg(path);
    run(command, authorization, timeout)
}

fn run(mut command: Command, authorization: &Authorization, timeout: Duration) -> Result<()> {
    authorization.control.check()?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut wrapped = CommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(process_wrap::std::ProcessGroup::leader());
    #[cfg(windows)]
    wrapped
        .wrap(process_wrap::std::JobObject)
        .wrap(process_wrap::std::CreationFlags(
            windows::Win32::System::Threading::CREATE_NO_WINDOW,
        ));
    let mut process = Process(
        wrapped
            .spawn()
            .context("无法启动 sudo，请确认已安装 sudo")?,
    );
    let child = &mut process.0;
    // The bounded password fits the pipe without waiting for the process to consume it.
    let mut input = child.stdin().take().context("无法打开 sudo 密码通道")?;
    input
        .write_all(authorization.password.0.as_bytes())
        .context("sudo 密码通道已关闭")?;
    input.write_all(b"\n").context("sudo 密码通道已关闭")?;
    drop(input);
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "sudo 写入失败，密码错误或权限不足（退出码 {}）",
                status.code().unwrap_or(-1)
            );
            return authorization.control.check();
        }
        if authorization.control.cancelled() || Instant::now() >= deadline {
            child.kill()?;
            authorization.control.check()?;
            anyhow::bail!("sudo 写入超时，请重试");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Process(Box<dyn process_wrap::std::ChildWrapper>);
impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
    }
}

#[cfg(test)]
mod tests;
