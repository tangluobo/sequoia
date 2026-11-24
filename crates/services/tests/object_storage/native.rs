use super::*;
use anyhow::{Context, ensure};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[tokio::test]
#[ignore = "requires TOMATO_FILE_DROP_EXE and TOMATO_FILE_DROP_OUTPUT; own native GPU windows"]
async fn capture_native_file_drop() -> Result<()> {
    use tomato_core::Store;
    use tomato_core::repository::{ConnectionFileRepository, WorkspaceRepository};
    let executable = PathBuf::from(
        std::env::var_os("TOMATO_FILE_DROP_EXE").context("Missing native executable")?,
    );
    let output = PathBuf::from(
        std::env::var_os("TOMATO_FILE_DROP_OUTPUT").context("Missing native output directory")?,
    );
    ensure!(
        executable.is_file() && output.is_dir(),
        "Native upload paths missing"
    );
    let cases: Vec<(&str, &str, &str)> =
        if std::env::var("TOMATO_FILE_DROP_MANUAL").as_deref() == Ok("1") {
            vec![("S3", "LIST", "manual")]
        } else {
            vec![
                ("S3", "LIST", "complete"),
                ("S3", "ICON", "complete"),
                ("S3", "COLUMN", "complete"),
                ("ALIYUN_OSS", "ICON", "complete"),
                ("LOCAL_DIRECTORY", "LIST", "complete"),
                ("S3", "LIST", "progress"),
                ("S3", "LIST", "cancel"),
            ]
        };
    let mut proofs = Vec::new();
    for (kind, view, case) in cases {
        let name = format!("{}-{}-{case}", kind.to_lowercase(), view.to_lowercase());
        let profile = tempfile::Builder::new()
            .prefix("tomato-file-drop-fixture-")
            .tempdir_in(&output)?;
        let source = profile.path().join("source");
        std::fs::create_dir_all(source.join("投递目录/嵌套"))?;
        std::fs::create_dir_all(source.join("投递目录/空目录"))?;
        let binary: Vec<_> = (0..270_000).map(|n| (n % 251) as u8).collect();
        std::fs::write(source.join("投递目录/嵌套/数据.bin"), &binary)?;
        std::fs::write(source.join("中文.txt"), "拖入上传")?;
        std::fs::write(source.join("zero.txt"), [])?;
        if matches!(case, "progress" | "cancel") {
            let large = std::fs::File::create(source.join("large.bin"))?;
            large.set_len(7 * 1024 * 1024)?;
        }
        let server = Server::start().await?;
        server.gate.hold.store(
            matches!(case, "progress" | "cancel"),
            std::sync::atomic::Ordering::Release,
        );
        server.state.lock().unwrap().files.insert(
            ("fixture".into(), "drop/existing.txt".into()),
            b"existing".to_vec(),
        );
        let remote = profile.path().join("remote");
        std::fs::create_dir_all(&remote)?;
        std::fs::write(remote.join("existing.txt"), "existing")?;
        let mut connection = if kind == "LOCAL_DIRECTORY" {
            let mut connection = Connection::new(kind);
            connection.set("localDirectoryPath", remote.to_string_lossy().to_string());
            connection
        } else {
            server.connection(kind)
        };
        connection.name = "文件拖放验收".into();
        connection.set("futureUploadFixture", serde_json::json!({"keep":true}));
        let repository = ConnectionFileRepository::new(profile.path().join("connections.json"));
        repository.load()?;
        repository.save(&Store {
            connections: vec![connection],
            ..Default::default()
        })?;
        let before = std::fs::read(profile.path().join("connections.json"))?;
        let proof = output.join(format!("{name}.state.json"));
        let png = output.join(format!("{name}.png"));
        let stdout = std::fs::File::create(output.join(format!("{name}.stdout.log")))?;
        let stderr = std::fs::File::create(output.join(format!("{name}.stderr.log")))?;
        let mut command = std::process::Command::new(&executable);
        command
            .env("TOMATO_DATA_DIR", profile.path())
            .env("TOMATO_UI_PREVIEW", "file_drop")
            .env("TOMATO_FILE_DROP_CASE", case)
            .env("TOMATO_FILE_DROP_VIEW", view)
            .env(
                "TOMATO_FILE_DROP_TARGET",
                if kind == "LOCAL_DIRECTORY" {
                    remote.to_string_lossy().replace('\\', "/")
                } else {
                    "/fixture/drop".into()
                },
            )
            .env("TOMATO_FILE_DROP_PROOF", &proof)
            .stdout(stdout)
            .stderr(stderr);
        if case == "manual" {
            // Preview installation is gated by CAPTURE; leave enough time for manual input.
            command
                .env("TOMATO_UI_CAPTURE", &png)
                .env("TOMATO_UI_CAPTURE_FRAMES", "1")
                .env("TOMATO_UI_CAPTURE_DELAY_MS", "600000");
            println!("Manual upload source: {}", source.display());
        } else {
            command
                .env("TOMATO_UI_CAPTURE", &png)
                .env("TOMATO_UI_CAPTURE_FRAMES", "1")
                .env("TOMATO_UI_CAPTURE_DELAY_MS", "10000");
        }
        let mut child = Child(command.spawn()?);
        let deadline =
            Instant::now() + Duration::from_secs(if case == "manual" { 600 } else { 90 });
        loop {
            if case == "cancel"
                && server
                    .gate
                    .entered
                    .load(std::sync::atomic::Ordering::Relaxed)
                    > 0
            {
                std::fs::write(profile.path().join("cancel-ready"), [])?;
            }
            if case == "manual"
                && proof.is_file()
                && let Ok(state) = std::fs::read(&proof)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| Ok(serde_json::from_slice::<serde_json::Value>(&bytes)?))
                && state["ready"] == true
            {
                std::fs::write(profile.path().join("stop"), [])?;
            }
            if let Some(status) = child.0.try_wait()? {
                ensure!(
                    status.success(),
                    "Native upload case {name} failed: {status}"
                );
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Native upload case {name} timed out"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&proof).with_context(|| format!("Missing proof for {name}"))?,
        )?;
        ensure!(
            state["ready"] == true
                && state["started"] == true
                && state["uiFileReads"] == 0
                && state["savedHostsContacted"] == false
                && state["systemClipboardWritten"] == false,
            "Native upload proof differs: {name}: {state}"
        );
        ensure!(
            before == std::fs::read(profile.path().join("connections.json"))?,
            "Legacy fixture configuration rewritten"
        );
        let dimensions = if case == "manual" {
            None
        } else {
            let image = image::open(&png)?;
            Some((image.width(), image.height()))
        };
        if matches!(case, "complete" | "manual") {
            ensure!(
                state["phase"] == "Finished"
                    && state["total"] == 6
                    && state["succeeded"] == 6
                    && state["failed"] == 0
                    && state["pending"] == false,
                "Native completion differs: {state}"
            );
            for (relative, expected) in [
                ("投递目录/嵌套/数据.bin", binary.as_slice()),
                ("中文.txt", "拖入上传".as_bytes()),
                ("zero.txt", &[][..]),
            ] {
                let actual = if kind == "LOCAL_DIRECTORY" {
                    std::fs::read(remote.join(relative))?
                } else {
                    server
                        .state
                        .lock()
                        .unwrap()
                        .files
                        .get(&("fixture".into(), format!("drop/{relative}")))
                        .cloned()
                        .with_context(|| format!("Missing uploaded object {relative}"))?
                };
                ensure!(
                    actual == expected,
                    "Native uploaded contents differ: {relative}"
                );
            }
            if kind == "LOCAL_DIRECTORY" {
                ensure!(
                    remote.join("投递目录/空目录").is_dir(),
                    "Empty folder missing"
                );
            } else {
                ensure!(
                    server
                        .state
                        .lock()
                        .unwrap()
                        .files
                        .contains_key(&("fixture".into(), "drop/投递目录/空目录/".into())),
                    "Empty object folder missing"
                );
            }
            for name in ["投递目录", "中文.txt", "zero.txt"] {
                ensure!(
                    state["entries"]
                        .as_array()
                        .is_some_and(|entries| entries.iter().any(|entry| entry == name)),
                    "Automatic list refresh lost {name}"
                );
            }
            ensure!(
                state["status"]
                    .as_str()
                    .is_some_and(|status| status.starts_with("上传完成")),
                "Completion notice lost during refresh"
            );
        } else {
            ensure!(
                server
                    .gate
                    .entered
                    .load(std::sync::atomic::Ordering::Relaxed)
                    > 0,
                "Multipart transfer was not reached"
            );
            ensure!(
                server.state.lock().unwrap().uploads.is_empty(),
                "Cancelled multipart upload was not aborted"
            );
            ensure!(
                !server
                    .state
                    .lock()
                    .unwrap()
                    .files
                    .contains_key(&("fixture".into(), "drop/large.bin".into())),
                "Cancelled upload was committed"
            );
            if case == "cancel" {
                ensure!(
                    state["phase"] == "Cancelled"
                        && state["cancelButtonUsed"] == true
                        && !state["uploading"].as_bool().unwrap_or(true),
                    "Actual cancel button did not finish: {state}"
                );
            } else {
                ensure!(
                    state["phase"] == "Copying" && dimensions == Some((484, 151)),
                    "Progress window differs: {state}"
                );
            }
        }
        proofs.push(serde_json::json!({"case":name,"normalExit":true,"state":state,"png":if case=="manual"{None}else{Some(&png)},"dimensions":dimensions,"legacyConfigurationUnchanged":true,"fileContentsVerified":true,"multipartCleanupVerified":matches!(case,"progress"|"cancel")}));
    }
    std::fs::write(
        output.join("validation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"passed","executable":executable,"cases":proofs,"systemClipboardWritten":false,"savedHostsContacted":false,"osInputUsed":std::env::var("TOMATO_FILE_DROP_MANUAL").as_deref()==Ok("1")}),
        )?,
    )?;
    Ok(())
}
