use super::*;
#[tokio::test]
#[ignore = "Set TOMATO_COPY_CAPTURE_EXE and TOMATO_COPY_CAPTURE_PATH; isolated native child viewport, no system clipboard writes"]
async fn native_copy_progress_and_app_exit_abort_a_real_upload_without_changing_shared_config()
-> Result<()> {
    let executable = std::env::var_os("TOMATO_COPY_CAPTURE_EXE")
        .ok_or_else(|| anyhow::anyhow!("native copy executable missing"))?;
    let image = std::path::PathBuf::from(
        std::env::var_os("TOMATO_COPY_CAPTURE_PATH")
            .ok_or_else(|| anyhow::anyhow!("native copy image path missing"))?,
    );
    let shared = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .map(|p| p.join(".tomato/connections.json"));
    let before = shared.as_ref().and_then(|p| std::fs::read(p).ok());
    let (server, pause, mut source, mut target, mut selection) = fixture().await?;
    {
        let mut state = server.state.lock().unwrap();
        state.buckets.clear();
        state.buckets.extend([
            "tomato-fixture-source".into(),
            "tomato-fixture-target".into(),
        ]);
        let bytes = state
            .files
            .remove(&("from".into(), "large.bin".into()))
            .unwrap();
        state
            .files
            .insert(("tomato-fixture-source".into(), "large.bin".into()), bytes);
    }
    source.name = "对象复制验收-源".into();
    source.set("bucket", "tomato-fixture-source");
    target.name = "对象复制验收-目标".into();
    target.set("bucket", "tomato-fixture-target");
    selection.bucket = "tomato-fixture-source".into();
    let profile = tempfile::Builder::new()
        .prefix("tomato-copy-fixture-")
        .tempdir()?;
    let config = profile.path().join("connections.json");
    tomato_core::Store {
        connections: vec![source, target],
        ..Default::default()
    }
    .save(&config)?;
    let config_before = std::fs::read(&config)?;
    let mut command = std::process::Command::new(executable);
    command
        .env("TOMATO_DATA_DIR", profile.path())
        .env("TOMATO_UI_CAPTURE", &image)
        .env("TOMATO_UI_CAPTURE_FRAMES", "8")
        .env("TOMATO_UI_PREVIEW", "object_copy")
        .env("TOMATO_UI_COPY_SELECTION", selection.text()?)
        .env("WGPU_BACKEND", "vulkan");
    command.stdout(std::process::Stdio::from(std::fs::File::create(
        image.with_extension("stdout.log"),
    )?));
    command.stderr(std::process::Stdio::from(std::fs::File::create(
        image.with_extension("stderr.log"),
    )?));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut child = Child(command.spawn()?);
    let mut close_seen = None;
    let end = Instant::now() + Duration::from_secs(45);
    let exit = loop {
        if close_seen.is_none() && image.with_extension("close").is_file() {
            close_seen = Some(Instant::now());
        }
        if let Some(exit) = child.0.try_wait()? {
            break exit;
        }
        anyhow::ensure!(Instant::now() < end, "native progress capture timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    anyhow::ensure!(exit.success(), "native progress capture exited with {exit}");
    anyhow::ensure!(close_seen.is_some(), "missing native close marker");
    let close_millis = close_seen.unwrap().elapsed().as_millis();
    anyhow::ensure!(
        close_millis < 3000,
        "multipart shutdown exceeded three seconds: {close_millis} ms"
    );
    let last = image.with_file_name(format!(
        "{}-07.png",
        image.file_stem().unwrap().to_string_lossy()
    ));
    anyhow::ensure!(
        last.exists(),
        "last native child-viewport screenshot missing"
    );
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(image.with_extension("state.json"))?)?;
    assert_eq!(state["phase"], "Copying");
    assert_eq!(state["stage"], "上传");
    assert_eq!(state["total"], 1);
    assert!(
        pause.entered.load(Ordering::Acquire) > 0,
        "the window must represent a live multipart upload"
    );
    assert!(
        server.state.lock().unwrap().uploads.is_empty(),
        "app exit must await AbortMultipartUpload while its runtime is alive"
    );
    assert_eq!(server.state.lock().unwrap().files.len(), 1);
    std::fs::write(
        image.with_extension("exit.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "normalExit":true,"closeToExitMillis":close_millis,"multipartAborted":true,
            "savedHostsContacted":false,"scope":"owned native application closing during a paused loopback multipart upload; no OS input injection"
        }))?,
    )?;
    assert_eq!(std::fs::read(config)?, config_before);
    if let Some(shared) = shared {
        anyhow::ensure!(
            std::fs::read(shared).ok() == before,
            "shared config changed during native fixture"
        );
    }
    pause.release.notify_waiters();
    println!("Native copy phase: {state}");
    Ok(())
}
