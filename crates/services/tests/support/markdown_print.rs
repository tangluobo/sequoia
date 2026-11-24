//! Drive the real PDF toolbar against a single disposable loopback connection.
use anyhow::{Context, Result, ensure};
use std::{path::PathBuf, time::Duration};
use tomato_core::{Connection, Store};

pub async fn capture(connection: Connection, mode: &str) -> Result<()> {
    let program =
        std::env::var_os("TOMATO_REMOTE_PRINT_EXE").context("Missing capture executable")?;
    let capture = PathBuf::from(
        std::env::var_os("TOMATO_REMOTE_PRINT_CAPTURE").context("Missing capture path")?,
    );
    let pdf = capture.with_extension("pdf");
    let report = capture.with_extension("print.json");
    ensure!(
        !capture.exists() && !pdf.exists() && !report.exists(),
        "Use fresh output paths"
    );
    let profile = tempfile::Builder::new()
        .prefix("tomato-markdown-print-fixture-")
        .tempdir()?;
    let config = profile.path().join("connections.json");
    Store {
        connections: vec![connection],
        ..Default::default()
    }
    .save(&config)?;
    let before = std::fs::read(&config)?;
    let mut child = tokio::process::Command::new(program)
        .kill_on_drop(true)
        .env("TOMATO_DATA_DIR", profile.path())
        .env("TOMATO_UI_CAPTURE", &capture)
        .env("TOMATO_UI_CAPTURE_FRAMES", "1")
        .env("TOMATO_UI_PREVIEW", "markdown_print")
        .env("TOMATO_MARKDOWN_PRINT_TEST", mode)
        .env("WGPU_BACKEND", "vulkan")
        .spawn()?;
    let status = tokio::time::timeout(Duration::from_secs(90), child.wait()).await??;
    ensure!(
        status.success(),
        "Remote print fixture exited with {status}"
    );
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    ensure!(
        result["passed"] == true && result["cancelled"] == false,
        "Print failed: {result}"
    );
    let message = result["message"].as_str().context("Missing print status")?;
    ensure!(
        message.contains("README.md（1 页）") && !message.contains("图片无法加载"),
        "{message}"
    );
    let source = profile.path().join("printed.pdf");
    ensure!(
        source.is_file() && capture.is_file(),
        "Expected a native page and capture"
    );
    std::fs::copy(source, pdf)?;
    ensure!(
        before == std::fs::read(config)?,
        "Remote print altered the owned fixture configuration"
    );
    Ok(())
}
