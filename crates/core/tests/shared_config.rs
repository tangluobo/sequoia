use anyhow::Result;
use tomato_core::{
    Store, legacy,
    repository::{ConnectionFileRepository, WorkspaceRepository},
};

const JAVA: &str = include_str!("fixtures/java-connections.enc");

#[test]
fn original_java_file_roundtrips_ids_passwords_and_unknown_fields() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    std::fs::write(&path, JAVA)?;
    let repository = ConnectionFileRepository::new(&path);
    let mut store = repository.load()?;
    assert_eq!(store.connections[0].id, "java-group");
    assert_eq!(
        store.connections[1].parent_id.as_deref(),
        Some("java-group")
    );
    assert_eq!(
        store.connections[1].text("password"),
        "fixture-only-password"
    );
    assert_eq!(
        store.connections[1].text("sshTunnelPassword"),
        "fixture-only-tunnel"
    );
    assert_eq!(
        store.connections[1].options["serverConfig"]["futureOption"],
        42
    );
    assert_eq!(std::fs::read_to_string(&path)?, JAVA);
    assert!(!repository.backup_path().exists());
    store.connections[1].name = "Rust edited".into();
    repository.save(&store)?;
    assert_eq!(std::fs::read_to_string(repository.backup_path())?, JAVA);
    let written = std::fs::read_to_string(&path)?;
    assert!(written.starts_with(legacy::MARKER));
    assert!(!written.contains("fixture-only-password"));
    let loaded = Store::load(&path)?;
    assert_eq!(
        serde_json::to_value(&loaded.connections)?,
        serde_json::to_value(&store.connections)?
    );
    repository.save(&store)?;
    assert_eq!(std::fs::read_to_string(repository.backup_path())?, JAVA);
    Ok(())
}

#[test]
fn external_writes_and_deletions_are_not_overwritten() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    std::fs::write(&path, JAVA)?;
    let repository = ConnectionFileRepository::new(&path);
    let store = repository.load()?;
    std::fs::write(&path, "[]")?;
    assert!(repository.save(&store).is_err());
    assert_eq!(std::fs::read_to_string(&path)?, "[]");
    assert!(!repository.backup_path().exists());
    std::fs::remove_file(&path)?;
    assert!(repository.save(&store).is_err());
    assert!(!path.exists());
    let store = repository.load()?;
    std::fs::write(&path, JAVA)?;
    assert!(repository.save(&store).is_err());
    assert_eq!(std::fs::read_to_string(&path)?, JAVA);
    Ok(())
}

#[test]
fn older_plaintext_variants_get_encrypted_backups_and_corrupt_files_are_untouched() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let old = legacy::decode(JAVA)?;
    std::fs::write(&path, serde_json::to_vec(&old)?)?;
    let repository = ConnectionFileRepository::new(&path);
    let store = repository.load()?;
    repository.save(&store)?;
    let backup = std::fs::read_to_string(repository.backup_path())?;
    assert!(backup.starts_with(legacy::MARKER));
    assert_eq!(
        legacy::decode(&backup)?[1].text("password"),
        "fixture-only-password"
    );
    std::fs::write(&path, "TOMATO_ENCRYPTEDcorrupt")?;
    let invalid = ConnectionFileRepository::new(&path);
    assert!(invalid.load().is_err());
    assert!(invalid.save(&store).is_err());
    assert_eq!(std::fs::read_to_string(&path)?, "TOMATO_ENCRYPTEDcorrupt");
    std::fs::write(&path, r#"{"unrelatedSetting":true}"#)?;
    assert!(ConnectionFileRepository::new(&path).load().is_err());
    Ok(())
}

#[test]
fn backup_failure_cancels_save() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    std::fs::write(&path, JAVA)?;
    let repository = ConnectionFileRepository::new(&path);
    let mut store = repository.load()?;
    store.connections[1].name = "edited".into();
    std::fs::create_dir(repository.backup_path())?;
    assert!(repository.save(&store).is_err());
    assert_eq!(std::fs::read_to_string(path)?, JAVA);
    Ok(())
}

#[test]
fn redacted_export_cannot_replace_the_shared_config() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    std::fs::write(&path, JAVA)?;
    let repository = ConnectionFileRepository::new(&path);
    let store = repository.load()?;
    assert!(repository.export(&store, &path).is_err());
    assert_eq!(std::fs::read_to_string(&path)?, JAVA);
    let export = directory.path().join("export.json");
    repository.export(&store, &export)?;
    assert!(!std::fs::read_to_string(export)?.contains("fixture-only-password"));
    Ok(())
}
