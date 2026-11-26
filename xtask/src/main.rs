use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("package") {
        bail!(
            "Usage: cargo xtask package [--target <triple>] [--profile release|dev] [--no-build] [--installer]"
        );
    }
    let mut target = None;
    let mut build = true;
    let mut installer = false;
    let mut profile = String::from("release");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => target = Some(args.next().context("--target needs a triple")?),
            "--no-build" => build = false,
            "--installer" => installer = true,
            "--profile" => profile = args.next().context("--profile needs release or dev")?,
            _ => bail!("unknown argument {arg}"),
        }
    }
    if !matches!(profile.as_str(), "release" | "dev") {
        bail!("--profile must be release or dev");
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root missing")?
        .to_path_buf();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(&cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()?;
    if !metadata.status.success() {
        bail!("cargo metadata failed");
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let version = metadata["packages"]
        .as_array()
        .context("missing packages")?
        .iter()
        .find(|p| p["name"] == "tomato")
        .and_then(|p| p["version"].as_str())
        .context("missing tomato version")?;
    let host = if let Some(target) = &target {
        target.clone()
    } else {
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| {
            Path::new(&cargo)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| {
                    p.join(if cfg!(windows) { "rustc.exe" } else { "rustc" })
                        .into_os_string()
                })
                .unwrap_or_else(|| "rustc".into())
        });
        let version = Command::new(rustc).arg("-vV").output()?;
        String::from_utf8(version.stdout)?
            .lines()
            .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
            .context("missing rustc host")?
    };
    if installer && (!cfg!(windows) || !host.contains("windows")) {
        bail!("--installer requires a Windows host and Windows target");
    }
    if build {
        let mut command = Command::new(&cargo);
        command
            .args([
                "build",
                "--locked",
                "--profile",
                &profile,
                "-p",
                "tomato",
                "-p",
                "tomato-cli",
            ])
            .current_dir(&root);
        if let Some(target) = &target {
            command.args(["--target", target]);
        }
        if !command.status()?.success() {
            bail!("release build failed");
        }
    }
    let target_directory = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .context("target_directory missing")?,
    );
    let profile_directory = if profile == "dev" { "debug" } else { "release" };
    let binaries = match &target {
        Some(target) => target_directory.join(target).join(profile_directory),
        None => target_directory.join(profile_directory),
    };
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let suffix = if profile == "dev" { "-debug" } else { "" };
    let platforms: serde_json::Value =
        serde_json::from_str(include_str!("../../packaging/targets.json"))?;
    let platform = platforms[&host].as_str().unwrap_or(&host);
    let name = format!("tomato-{version}-{platform}{suffix}");
    let stage = dist.join(&name);
    // Only ever replace this exact generated staging directory within dist.
    if stage.exists() {
        let resolved = fs::canonicalize(&stage)?;
        let base = fs::canonicalize(&dist)?;
        if resolved.parent() != Some(base.as_path()) {
            bail!("staging path escapes dist");
        }
        fs::remove_dir_all(&resolved)?;
    }
    fs::create_dir(&stage)?;
    let suffix = if host.contains("windows") { ".exe" } else { "" };
    for binary in ["tomato", "tomato-cli"] {
        fs::copy(
            binaries.join(format!("{binary}{suffix}")),
            stage.join(format!("{binary}{suffix}")),
        )
        .with_context(|| format!("missing release binary {binary}"))?;
    }
    for file in [
        "README.md",
        "LICENSE",
        "LICENSE-TOMATO-ASSETS",
        "docs/asset-notices.md",
        "docs/third-party/access-export-NOTICES.txt",
        "docs/third-party/data-import-NOTICES.txt",
        "docs/data-import.md",
        "vendor/odbc-import/README.md",
        "vendor/odbc-import/odbc.c",
        "scripts/build-odbc32.py",
        "vendor/access-export/TomatoAccess.java",
        "vendor/access-export/README.md",
        "docs/performance.md",
        "docs/configuration.md",
        "packaging/measure-memory.ps1",
        "docs/migration.md",
        "docs/architecture.md",
        "docs/remote-protocols.md",
        "docs/rdp-channels.md",
        "licenses/mimalloc.txt",
        "vendor/virtual-file-transfer/LICENSE.md",
        "crates/services/assets/adb/LICENSE-scrcpy.txt",
        "crates/services/assets/adb/NOTICE-scrcpy.txt",
        "docs/third-party/libadb-LICENSE-MIT.txt",
        "docs/third-party/openh264-NOTICES.txt",
        "docs/third-party/music-playback-NOTICES.txt",
        "docs/third-party/rdp-security-NOTICES.txt",
        "docs/third-party/rdp-security-LICENSE-MIT.txt",
        "docs/third-party/ironrdp-connector-LICENSE-MIT.txt",
        "docs/third-party/ironrdp-connector-LICENSE-APACHE.txt",
        "docs/third-party/ironrdp-connector-PATCH.md",
        "docs/third-party/ironrdp-acceptor-PATCH.md",
        "docs/third-party/zmodem2-LICENSE-MIT.txt",
        "docs/third-party/zmodem2-LICENSE-APACHE.txt",
        "docs/third-party/zmodem2-PATCH.md",
        "docs/third-party/clipboard-win-LICENSE.txt",
        "docs/third-party/clipboard-rs-LICENSE.txt",
        "docs/third-party/russh-sftp-LICENSE.txt",
        "docs/third-party/russh-sftp-PATCH.md",
        "docs/third-party/storage-NOTICES.txt",
        "docs/third-party/smb-server-LICENSE.txt",
        "docs/third-party/hive-NOTICES.txt",
        "docs/third-party/nsis-NOTICES.txt",
        "docs/third-party/nsis-COPYING.txt",
        "vendor/hive-protocol/LICENSE",
        "vendor/hive-protocol/source.json",
        "docs/third-party/bittorrent-NOTICES.txt",
        "vendor/librqbit/LICENSE",
        "vendor/librqbit/TOMATO_PATCHES.json",
        "docs/third-party/vnc-NOTICES.txt",
        "docs/third-party/vt100-LICENSE.txt",
        "docs/third-party/vt100-PATCH.md",
        "docs/third-party/eframe-MIT.txt",
        "docs/third-party/eframe-APACHE.txt",
        "docs/third-party/eframe-PATCH.md",
        "docs/third-party/egui-winit-PATCH.md",
        "docs/third-party/egui-PATCH.md",
        "docs/third-party/mongodb-LICENSE-APACHE.txt",
        "docs/third-party/mongodb-NOTICES.txt",
        "docs/third-party/pulldown-cmark-LICENSE.txt",
        "docs/third-party/fontawesome-LICENSE.txt",
        "docs/third-party/sqlx-core-LICENSE-MIT.txt",
        "docs/third-party/sqlx-core-LICENSE-APACHE.txt",
        "docs/third-party/sqlx-core-PATCH.md",
        "docs/third-party/sqlx-mysql-LICENSE-MIT.txt",
        "docs/third-party/sqlx-mysql-LICENSE-APACHE.txt",
        "docs/third-party/sqlx-postgres-LICENSE-MIT.txt",
        "docs/third-party/sqlx-postgres-LICENSE-APACHE.txt",
        "docs/third-party/sqlx-drivers-PATCH.md",
        "crates/ui-egui/assets/upstream/openjfx/LICENSE",
        "crates/ui-egui/assets/upstream/openjfx/ADDITIONAL_LICENSE_INFO",
        "crates/ui-egui/assets/upstream/openjfx/ASSEMBLY_EXCEPTION",
        "crates/ui-egui/assets/upstream/openjfx/NOTICE.md",
        "docs/source-inventory.json",
    ] {
        let destination = stage.join(file);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(root.join(file), destination)?;
    }
    if host.contains("apple") {
        let contents = stage.join("Tomato.app/Contents");
        fs::create_dir_all(contents.join("MacOS"))?;
        fs::copy(binaries.join("tomato"), contents.join("MacOS/tomato"))?;
        fs::write(
            contents.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>tomato</string><key>CFBundleIdentifier</key><string>com.tangluobo.tomato</string><key>CFBundleName</key><string>Tomato</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundlePackageType</key><string>APPL</string><key>NSHighResolutionCapable</key><true/></dict></plist>"
            ),
        )?;
    }
    let mut paths = Vec::new();
    collect(&stage, &mut paths)?;
    paths.sort();
    let archive = dist.join(format!("{name}.zip"));
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    for path in &paths {
        let relative = path
            .strip_prefix(&dist)?
            .to_string_lossy()
            .replace('\\', "/");
        let executable = path.extension().is_none()
            && path
                .file_name()
                .is_some_and(|name| name == "tomato" || name == "tomato-cli");
        zip.start_file(
            relative,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(if executable { 0o755 } else { 0o644 }),
        )?;
        std::io::copy(&mut fs::File::open(path)?, &mut zip)?;
    }
    zip.finish()?;
    checksum(&archive)?;
    let archive = dist.join(format!("{name}.tar.gz"));
    let encoder =
        flate2::write::GzEncoder::new(fs::File::create(&archive)?, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all(&name, &stage)?;
    tar.into_inner()?.finish()?;
    checksum(&archive)?;
    if installer {
        let output = dist.join(format!("{name}-setup.exe"));
        let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python".into());
        if !Command::new(python)
            .arg(root.join("scripts/build-installer.py"))
            .arg("--stage")
            .arg(&stage)
            .arg("--version")
            .arg(version)
            .arg("--output")
            .arg(output)
            .current_dir(&root)
            .status()?
            .success()
        {
            bail!("Windows installer build failed");
        }
    }
    println!("Packaged {}", stage.display());
    Ok(())
}

fn collect(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(&path, paths)?;
        } else {
            paths.push(path);
        }
    }
    Ok(())
}
fn checksum(path: &Path) -> Result<()> {
    let mut digest = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let checksum = format!(
        "{:x}  {}\n",
        digest.finalize(),
        path.file_name()
            .context("archive filename missing")?
            .to_string_lossy()
    );
    let mut output = fs::File::create(format!("{}.sha256", path.display()))?;
    output.write_all(checksum.as_bytes())?;
    Ok(())
}
