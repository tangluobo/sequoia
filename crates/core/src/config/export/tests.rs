use super::*;
fn fixture() -> Result<Vec<Connection>> {
    crate::legacy::decode(
        r#"[{"id":"group","name":"目录"},{"id":"nested","name":"子目录","parentId":"group"},{"id":"db","name":"数据库","type":"MYSQL","parentId":"nested","password":"fixture-password","sshTunnelPassword":"fixture-tunnel","future":{"token":"fixture-token","keep":17}},{"id":"other","type":"SSH","name":"未选连接","password":"unselected-secret"},{"id":"empty","name":"空目录"}]"#,
    )
}
#[test]
fn tri_state_selection_retains_ancestors_and_does_not_export_unselected_credentials() -> Result<()>
{
    let nodes = fixture()?;
    let original = serde_json::to_value(&nodes)?;
    let mut selection = Selection::default();
    assert!(selection.prepare(&nodes, Credentials::Redacted).is_err());
    let tree = selection.tree(&nodes);
    selection.set(&tree, 2, true);
    let checks: Vec<_> = selection
        .tree(&nodes)
        .rows
        .iter()
        .map(|r| r.check)
        .collect();
    assert_eq!(
        checks,
        [
            Check::Checked,
            Check::Checked,
            Check::Checked,
            Check::Unchecked,
            Check::Unchecked
        ]
    );
    let encoded = selection.prepare(&nodes, Credentials::Included)?.encode()?;
    assert!(!encoded.contains("fixture-password") && !encoded.contains("unselected-secret"));
    let exported = crate::legacy::decode(&encoded)?;
    assert_eq!(
        exported.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["group", "nested", "db"]
    );
    assert_eq!(exported[2].text("password"), "fixture-password");
    assert_eq!(exported[2].options["future"]["keep"], 17);
    let redacted =
        crate::legacy::decode(&selection.prepare(&nodes, Credentials::Redacted)?.encode()?)?;
    assert!(
        redacted[2].text("password").is_empty() && redacted[2].text("sshTunnelPassword").is_empty()
    );
    assert_ne!(redacted[2].options["future"]["token"], "fixture-token");
    let json = crate::legacy::decrypt(encoded.strip_prefix(crate::legacy::MARKER).unwrap())?;
    let envelope: Value = serde_json::from_str(&json)?;
    assert_eq!(envelope["format"], "tomato-connections-export");
    assert_eq!(envelope["version"], 1);
    assert_eq!(serde_json::to_value(&nodes)?, original);
    selection.all(&nodes, true);
    let tree = selection.tree(&nodes);
    selection.set(&tree, 2, false);
    assert_eq!(selection.tree(&nodes).rows[0].check, Check::Unchecked);
    assert_eq!(
        selection
            .prepare(&nodes, Credentials::Included)?
            .connections
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["other", "empty"]
    );
    Ok(())
}
#[test]
fn publication_cancellation_preserves_existing_target_and_active_configuration() -> Result<()> {
    use crate::repository::{ConnectionFileRepository, WorkspaceRepository, write_export_checked};
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("connections.json");
    let nodes = fixture()?;
    let store = Store {
        connections: nodes,
        ..Default::default()
    };
    store.save(&source)?;
    let original = std::fs::read(&source)?;
    let repository = ConnectionFileRepository::new(&source);
    repository.load()?;
    let mut selection = Selection::default();
    selection.all(&store.connections, true);
    let prepared = selection.prepare(&store.connections, Credentials::Included)?;
    assert!(
        repository
            .export_prepared(
                &prepared,
                &directory.path().join(".").join("connections.json")
            )
            .is_err()
    );
    assert_eq!(std::fs::read(&source)?, original);
    let target = directory.path().join("export.json");
    std::fs::write(&target, b"previous-export")?;
    let calls = std::cell::Cell::new(0);
    let error = write_export_checked(Some(&source), &target, &vec![b'x'; 128 * 1024], || {
        calls.set(calls.get() + 1);
        anyhow::ensure!(calls.get() < 4, "fixture cancellation");
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("cancellation"));
    assert_eq!(std::fs::read(&target)?, b"previous-export");
    assert_eq!(std::fs::read(&source)?, original);
    assert!(!repository.backup_path().exists());
    Ok(())
}
#[test]
fn serialized_default_policy_cannot_bypass_redaction_and_oversized_exports_are_bounded()
-> Result<()> {
    let mut prepared: Prepared =
        serde_json::from_value(serde_json::json!({"connections":fixture()?}))?;
    let content = prepared.encode()?;
    assert!(
        crate::legacy::decode(&content)?[2]
            .text("password")
            .is_empty()
    );
    assert!(!format!("{prepared:?}").contains("fixture-password"));
    prepared.connections[2].set("futureLargeValue", "x".repeat(25 * 1024 * 1024));
    assert!(
        prepared
            .encode()
            .unwrap_err()
            .to_string()
            .contains("32 MiB")
    );
    Ok(())
}
