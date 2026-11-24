//! Explicitly authorized GUI acceptance of the saved host; excluded from ordinary tests.
use anyhow::{Context, Result, ensure};
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use tomato_core::Store;

struct OwnedApp(std::process::Child);
impl Drop for OwnedApp {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
#[ignore = "requires explicit saved 110 authorization, desktop and a fresh optimized GUI"]
fn authorized_saved_110_native_tab_and_window() -> Result<()> {
    ensure!(
        std::env::var("TOMATO_TEST_SAVED_110").as_deref() == Ok("authorized"),
        "Saved-host acceptance was not enabled"
    );
    let executable =
        PathBuf::from(std::env::var_os("TOMATO_TEST_SAVED_110_EXE").context("Missing owned GUI")?);
    let directory = PathBuf::from(
        std::env::var_os("TOMATO_TEST_SAVED_110_GUI_OUTPUT")
            .context("Missing fresh output directory")?,
    );
    ensure!(
        executable.is_file() && !directory.exists(),
        "Expected an existing GUI and fresh output directory"
    );
    std::fs::create_dir_all(&directory)?;
    let config = Store::default_path();
    let before = std::fs::read(&config)?;
    // Copy only the encrypted connection repository. No DDNS, port mapping,
    // downloader or other scheduler state may be loaded during host acceptance.
    let profile = directory.join("profile");
    std::fs::create_dir(&profile)?;
    std::fs::write(profile.join("connections.json"), &before)?;
    let mut results = Vec::new();
    for mode in ["TAB", "WINDOW"] {
        let capture = directory.join(format!("{mode}.png"));
        let report = directory.join(format!("{mode}.acceptance.json"));
        let log = directory.join(format!("{mode}.stderr.log"));
        let mut app = OwnedApp(
            Command::new(&executable)
                .env("TOMATO_DATA_DIR", &profile)
                .env("TOMATO_UI_PREVIEW", "rdp")
                .env("TOMATO_UI_CAPTURE", &capture)
                .env("TOMATO_UI_CAPTURE_DELAY_MS", "10000")
                .env("TOMATO_RDP_CAPTURE_MODE", mode)
                .env("TOMATO_RDP_ACCEPTANCE_REPORT", &report)
                .env("TOMATO_CAPTURE_TRACE", "1")
                .env("TOMATO_RENDER_DIAGNOSTICS", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::from(std::fs::File::create_new(log)?))
                .spawn()?,
        );
        let started = Instant::now();
        let mut close_seen = None;
        let status = loop {
            if close_seen.is_none() && capture.with_extension("close").is_file() {
                close_seen = Some(Instant::now());
            }
            if let Some(status) = app.0.try_wait()? {
                break status;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(45),
                "Saved 110 {mode} GUI did not exit normally"
            );
            ensure!(
                close_seen.is_none_or(|close: Instant| close.elapsed() < Duration::from_secs(5)),
                "Saved 110 {mode} close request exceeded 5 seconds"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        ensure!(status.success(), "Saved 110 {mode} GUI failed: {status}");
        ensure!(
            close_seen.is_some(),
            "Missing close request marker for {mode}"
        );
        let close_millis = close_seen.unwrap().elapsed().as_millis();
        let image = image::open(&capture)?;
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
        let sessions = data["sessions"]
            .as_array()
            .context("Missing actual session report")?;
        ensure!(
            sessions.len() == 1
                && sessions[0]["mode"] == mode
                && sessions[0]["ready"] == true
                && sessions[0]["texturePresent"] == true,
            "Actual {mode} session was not rendered: {data}"
        );
        ensure!(
            sessions[0]["canvas"] == serde_json::json!([1920, 1080]),
            "Unexpected saved host canvas"
        );
        ensure!(
            before == std::fs::read(&config)?,
            "Shared legacy config changed externally"
        );
        results.push(serde_json::json!({"mode":mode,"normalExit":true,
            "captureSize":[image.width(),image.height()],
            "captureToExitMillis":close_millis,
            "configurationUnchanged":true,"report":data}));
    }
    let proof = serde_json::json!({"savedHost":"110","results":results,
        "actualNativeGuiConnected":true,"configurationUnchanged":true,
        "isolatedEncryptedProfile":true,"otherSchedulerStateCopied":false,
        "physicalMouseInteractionAccepted":false,
        "scope":"actual host rendered by the optimized application in both modes; physical input requires separate observation"});
    std::fs::write(
        directory.join("validation.json"),
        serde_json::to_vec_pretty(&proof)?,
    )?;
    println!(
        "Actual saved 110 TAB/WINDOW GUI rendering and normal exit passed; physical mouse interaction is not inferred"
    );
    Ok(())
}
