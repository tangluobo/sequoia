use anyhow::{Context, Result, ensure};
use std::{
    ops::Deref,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Fixture {
    process: Child,
    files: PathBuf,
    port: u16,
}
impl Fixture {
    pub fn start(directory: &Path) -> Result<Self> {
        let profile = PathBuf::from(
            std::env::var_os("TOMATO_DATA_DIR")
                .context("Set an isolated TOMATO_DATA_DIR before running ADB peer tests")?,
        );
        ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-adb-fixture-")),
            "Use a disposable tomato-adb-fixture-* data directory"
        );
        let exe = std::env::var_os("TOMATO_TEST_ADB_FIXTURE")
            .context("Set TOMATO_TEST_ADB_FIXTURE to the built adb_fixture example")?;
        let files = directory.join("adb-fixture");
        let mut command = Command::new(exe);
        command
            .arg(&files)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let process = command.spawn()?;
        let mut fixture = Self {
            process,
            files,
            port: 0,
        };
        let started = Instant::now();
        loop {
            if let Ok(port) = std::fs::read_to_string(fixture.with_extension("port"))
                && let Ok(port) = port.trim().parse()
                && port != 0
            {
                fixture.port = port;
                return Ok(fixture);
            }
            ensure!(
                fixture.process.try_wait()?.is_none(),
                "ADB peer exited during startup"
            );
            ensure!(
                started.elapsed() < Duration::from_secs(10),
                "ADB peer did not publish its loopback port"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn connection(&self) -> tomato_core::Connection {
        let mut connection = tomato_core::Connection::new("ADB");
        connection.set("host", "127.0.0.1");
        connection.set("port", self.port);
        connection
    }
}
impl Deref for Fixture {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.files
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
