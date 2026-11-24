use anyhow::Result;
use tomato_services::json_history::{Action, execute};
#[test]
fn legacy_history_streams_summaries_restores_copies_deletes_and_preserves_unknown_fields()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("json_history.json");
    let original = r#"[{"json":"{\"旧值\":1}","timestamp":"12:34:56","summary":"原版摘要","future":{"keep":true}}]"#;
    std::fs::write(&path, original)?;
    let loaded = execute(&path, Action::Load)?;
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(loaded.items[0].summary, "原版摘要");
    let key = loaded.items[0].key.clone();
    let restored = execute(&path, Action::Restore(key.clone()))?;
    assert_eq!(restored.input.as_deref(), Some("{\"旧值\":1}"));
    assert!(restored.formatted.is_some());
    assert_eq!(
        execute(&path, Action::Copy(key.clone()))?.clipboard,
        restored.input
    );
    let added = execute(
        &path,
        Action::Format {
            json: "{\"new\":2}".into(),
        },
    )?;
    assert_eq!(added.items.len(), 2);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(json[1]["future"]["keep"], true);
    assert_eq!(
        std::fs::read_to_string(path.with_extension("json.bak"))?,
        original
    );
    assert!(execute(&path, Action::Remove(key)).is_err());
    let before = std::fs::read(&path)?;
    let duplicate = execute(
        &path,
        Action::Format {
            json: "{\"new\": 2}".into(),
        },
    )?;
    assert_eq!(duplicate.items.len(), 2);
    assert_eq!(std::fs::read(&path)?, before);
    let removed = execute(&path, Action::Remove(duplicate.items[1].key.clone()))?;
    assert_eq!(removed.items.len(), 1);
    assert!(execute(&path, Action::Clear)?.items.is_empty());
    Ok(())
}
#[test]
fn history_retains_fifty_original_records_and_refuses_to_overwrite_corrupt_input() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("json_history.json");
    let entries:Vec<_>=(0..50).map(|index|serde_json::json!({"json":format!("{{\"index\":{index}}}"),"timestamp":"01:02:03","summary":format!("item {index}")})).collect();
    std::fs::write(&path, serde_json::to_vec(&entries)?)?;
    let output = execute(
        &path,
        Action::Format {
            json: "{\"index\":51}".into(),
        },
    )?;
    assert_eq!(output.items.len(), 50);
    assert_eq!(
        output.items.last().map(|i| i.summary.as_str()),
        Some("item 48")
    );
    let corrupt = b"[{ broken";
    std::fs::write(&path, corrupt)?;
    assert!(
        execute(&path, Action::Format { json: "{}".into() })?
            .notice
            .is_some()
    );
    assert_eq!(std::fs::read(&path)?, corrupt);
    Ok(())
}
