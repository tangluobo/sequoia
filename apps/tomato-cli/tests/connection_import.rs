use anyhow::Result;
use std::process::Command;
use tomato_core::Store;

#[test]
fn actual_cli_import_preserves_existing_ids_credentials_references_and_duplicate_noop() -> Result<()>
{
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("connections.json");
    let store = Store {
        connections: tomato_core::legacy::decode(
            r#"[{"id":"existing-folder","name":"folder"},{"id":"existing-ssh","name":"ssh","type":"SSH","password":"existing-fixture-only"}]"#,
        )?,
        ..Default::default()
    };
    store.save(&config)?;
    let before = std::fs::read(&config)?;
    let source = dir.path().join("import.json");
    std::fs::write(
        &source,
        r#"[{"id":"folder","name":"folder"},{"id":"ssh","name":"ssh","type":"SSH","password":"incoming-fixture-only"},{"id":"db","name":"db","type":"MYSQL","parentId":"folder","sshTunnelHostId":"ssh","future":{"keep":42}}]"#,
    )?;
    let input = std::fs::read(&source)?;
    for count in [1, 0] {
        let current = std::fs::read(&config)?;
        let output = Command::new(env!("CARGO_BIN_EXE_tomato-cli"))
            .arg("--config")
            .arg(&config)
            .arg("import")
            .arg(&source)
            .env("TOMATO_DATA_DIR", dir.path())
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)?.contains(&format!("已导入 {count} 个连接")));
        if count == 0 {
            assert_eq!(
                std::fs::read(&config)?,
                current,
                "duplicate import must leave the source byte-for-byte unchanged"
            );
        }
    }
    let loaded = Store::load(&config)?;
    assert_eq!(loaded.connections.len(), 3);
    assert_eq!(loaded.connections[0].id, "existing-folder");
    assert_eq!(
        loaded.connections[1].text("password"),
        "existing-fixture-only"
    );
    assert_eq!(
        loaded.connections[2].parent_id.as_deref(),
        Some("existing-folder")
    );
    assert_eq!(
        loaded.connections[2].text("sshTunnelHostId"),
        "existing-ssh"
    );
    assert_eq!(loaded.connections[2].options["future"]["keep"], 42);
    assert_eq!(
        std::fs::read(config.with_file_name("connections.json.before-rust.bak"))?,
        before
    );
    assert_eq!(std::fs::read(&source)?, input);
    assert!(!std::fs::read_to_string(config)?.contains("fixture-only"));
    Ok(())
}
