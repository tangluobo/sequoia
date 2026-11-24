use tomato_core::backup::{Definition, Metadata, Scope};
#[test]
fn original_nb3_metadata_round_trips_without_changing_navicat_field_names() -> anyhow::Result<()> {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/backup/original-format.json"))?;
    let meta: Metadata = serde_json::from_value(original["metadata"].clone())?;
    let definition: Definition = serde_json::from_value(original["definition"].clone())?;
    assert_eq!(serde_json::to_value(meta)?, original["metadata"]);
    assert_eq!(serde_json::to_value(definition)?, original["definition"]);
    assert_eq!(
        Scope {
            connection: "旧 连接".into(),
            database: "db".into(),
            path: "folder// /..".into()
        }
        .directory()?,
        std::path::Path::new("旧_连接/db/backup/folder/unnamed")
    );
    Ok(())
}
#[test]
#[cfg(windows)]
fn original_windows_backup_names_preserve_nbsp_and_reject_control_and_trailing_space()
-> anyhow::Result<()> {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/backup/original-format.json"))?;
    let scope = Scope {
        connection: "test".into(),
        database: "db".into(),
        path: String::new(),
    };
    for case in original["paths"].as_array().unwrap() {
        let file = scope.create_file(case.get("input").and_then(serde_json::Value::as_str));
        if let Some(expected) = case.get("filename").and_then(serde_json::Value::as_str) {
            assert_eq!(
                file?.file_name().unwrap().to_string_lossy(),
                expected,
                "{case}"
            );
        } else {
            assert!(file.is_err(), "{case}");
        }
    }
    Ok(())
}
