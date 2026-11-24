use tomato_core::queries::{Scope, sanitize};
#[test]
fn query_names_and_paths_match_actual_java_with_ascii_whitespace_and_nested_folders() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sql-query/original.json")).unwrap();
    for row in cases["names"].as_array().unwrap() {
        assert_eq!(
            sanitize(row["input"].as_str()),
            row["result"].as_str().unwrap()
        );
    }
    for row in cases["paths"].as_array().unwrap() {
        let scope = Scope {
            connection: "生产 A".into(),
            database: "my db".into(),
            path: row["path"].as_str().unwrap_or_default().into(),
        };
        assert_eq!(
            scope
                .directory()
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            row["relative"].as_str().unwrap()
        );
    }
    let unsafe_scope = Scope {
        connection: "production".into(),
        database: "main".into(),
        path: "../outside".into(),
    };
    assert!(unsafe_scope.directory().is_err());
}
