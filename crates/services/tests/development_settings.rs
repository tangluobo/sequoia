use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use tomato_core::development::runtime::EntryRole;
use tomato_services::development::{Action, Manager, Output};
async fn validate(
    manager: &mut Manager,
    config: &Path,
    role: EntryRole,
    kind: &str,
    path: &Path,
    settings: &Path,
) -> Result<Option<String>> {
    let Output::EntryValidated { message } = manager
        .execute(
            config.into(),
            Action::ValidateEntry {
                role,
                kind: kind.into(),
                name: "owned fixture".into(),
                path: path.into(),
                settings: settings.into(),
            },
        )
        .await?
    else {
        anyhow::bail!("unexpected validation output")
    };
    Ok(message)
}
async fn detect(manager: &mut Manager, config: &Path, kind: &str, path: &Path) -> Result<String> {
    let Output::Text(version) = manager
        .execute(
            config.into(),
            Action::DetectVersion {
                kind: kind.into(),
                path: path.into(),
            },
        )
        .await?
    else {
        anyhow::bail!("unexpected version output")
    };
    Ok(version)
}
#[tokio::test]
async fn sdk_validation_and_release_detection_use_real_files_without_saving_config() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let config = root.join("development.json");
    let mut manager = Manager::default();
    let missing = root.join("missing");
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::Runtime,
            "JDK",
            &missing,
            &missing
        )
        .await?
        .is_some()
    );
    let sdk = root.join("sdk");
    std::fs::create_dir(&sdk)?;
    std::fs::write(
        sdk.join("release"),
        "IMPLEMENTOR=\"owned fixture\"\r\nJAVA_VERSION=\"25.0.3\"\r\n",
    )?;
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::Runtime,
            "JDK",
            &sdk,
            &missing
        )
        .await?
        .is_none()
    );
    assert_eq!(detect(&mut manager, &config, "JDK", &sdk).await?, "25.0.3");
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::Runtime,
            "NODE",
            &sdk,
            &missing
        )
        .await?
        .is_some(),
        "Node requires a file, not an installation directory"
    );
    let node = sdk.join("not-an-executable");
    std::fs::write(&node, [])?;
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::Runtime,
            "NODE",
            &node,
            &missing
        )
        .await?
        .is_none()
    );
    assert_eq!(
        detect(&mut manager, &config, "NODE", &node).await?,
        "",
        "failed detection must leave an editable empty version without an error dialog"
    );
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::BuildTool,
            "MAVEN",
            &sdk,
            &missing
        )
        .await?
        .is_some()
    );
    let settings = root.join("settings.xml");
    std::fs::write(&settings, "<settings/>")?;
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::BuildTool,
            "MAVEN",
            &sdk,
            &settings
        )
        .await?
        .is_none()
    );
    ensure!(
        validate(
            &mut manager,
            &config,
            EntryRole::BuildTool,
            "GRADLE",
            &sdk,
            &missing
        )
        .await?
        .is_none()
    );
    assert!(
        !config.exists(),
        "validation/detection must not persist configuration"
    );
    Ok(())
}
fn script(root: &Path, name: &str, lines: &[&str]) -> Result<PathBuf> {
    #[cfg(windows)]
    let path = root.join(format!("{name}.cmd"));
    #[cfg(not(windows))]
    let path = root.join(name);
    #[cfg(windows)]
    let text = format!(
        "@echo off\r\n{}\r\n",
        lines
            .iter()
            .map(|line| format!("echo {line}"))
            .collect::<Vec<_>>()
            .join("\r\n")
    );
    #[cfg(not(windows))]
    let text = format!(
        "#!/bin/sh\n{}\n",
        lines
            .iter()
            .map(|line| format!("printf '%s\\n' '{line}'"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(&path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}
#[tokio::test]
async fn actual_version_commands_ignore_jvm_warnings_and_gradle_separators() -> Result<()> {
    let dir = tempfile::Builder::new()
        .prefix("tomato SDK & owned-")
        .tempdir()?;
    let root = dir.path();
    let config = root.join("development.json");
    let mut manager = Manager::default();
    let node = script(
        root,
        "node",
        &["WARNING: fixture startup message", "v22.15.0"],
    )?;
    assert_eq!(
        detect(&mut manager, &config, "NODE", &node).await?,
        "22.15.0"
    );
    let maven = script(
        root,
        "mvn",
        &[
            "WARNING: A terminally deprecated JVM method",
            "Apache Maven 3.9.1 (owned fixture)",
        ],
    )?;
    assert_eq!(
        detect(&mut manager, &config, "MAVEN", &maven).await?,
        "3.9.1 (owned fixture)"
    );
    let gradle = script(
        root,
        "gradle",
        &[
            "----------------------------------------",
            "Gradle 9.5.0",
            "----------------------------------------",
        ],
    )?;
    assert_eq!(
        detect(&mut manager, &config, "GRADLE", &gradle).await?,
        "9.5.0"
    );
    assert!(!config.exists());
    Ok(())
}
