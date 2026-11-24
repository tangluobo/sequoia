//! Only the user-selected Linux RDP connection; never run by ordinary regression.
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};
use tomato_core::Store;
use tomato_services::remote_desktop::{DesktopSession, Status};

#[test]
#[ignore = "requires explicit user-selected Linux RDP diagnosis and TOMATO_TEST_SAVED_LINUX=authorized"]
fn authorized_saved_linux_receives_desktop() -> Result<()> {
    ensure!(
        std::env::var("TOMATO_TEST_SAVED_LINUX").as_deref() == Ok("authorized"),
        "Saved Linux diagnosis not enabled"
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("TOMATO_TEST_SAVED_LINUX_OUTPUT")
            .context("Missing fresh diagnostic output")?,
    );
    ensure!(!output.exists(), "Diagnostic output must be fresh");
    let path = Store::default_path();
    let before = std::fs::read(&path)?;
    let store = Store::load(&path)?;
    let mut connection = store
        .connections
        .iter()
        .find(|c| {
            c.id == "d0c01833-407d-484e-a407-fc4632aabd5c"
                && c.kind() == "RDP"
                && c.name == "小主机桌面"
        })
        .context("User-selected Linux RDP connection not found")?
        .clone();
    // Avoid owning a native clipboard HWND or sound endpoint in a protocol-only probe.
    connection.set("mapClipboard", false);
    connection.set("enableSound", false);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut session = DesktopSession::start(&connection, runtime.handle())?;
    let start = Instant::now();
    let mut frames = 0;
    let mut failure = None;
    let mut size = None;
    while start.elapsed() < Duration::from_secs(15) {
        session.poll();
        if let Status::Failed(error) = session.status() {
            let secret = connection.text("password");
            failure = Some(if secret.is_empty() {
                error
            } else {
                error.replace(secret, "[redacted]")
            });
            break;
        }
        for update in session.take_rdp_updates() {
            frames += 1;
            size = Some([update.frame.width, update.frame.height]);
        }
        if frames > 0 && start.elapsed() > Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(session);
    ensure!(
        before == std::fs::read(&path)?,
        "Shared configuration changed externally"
    );
    let report = serde_json::json!({"savedConnection":"小主机桌面","frames":frames,"size":size,"elapsedMs":start.elapsed().as_millis(),"failure":failure,"configurationUnchanged":true,"passwordIncluded":false,"clipboardAccessed":false});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    ensure!(
        failure.is_none() && frames > 0,
        "Selected Linux RDP did not reach a framebuffer; see sanitized report"
    );
    Ok(())
}
