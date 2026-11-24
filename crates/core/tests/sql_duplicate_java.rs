fn scalar(text: &str, units: usize) -> usize {
    let mut count = 0;
    let mut seen = 0;
    for ch in text.chars() {
        if seen >= units {
            break;
        }
        seen += ch.len_utf16();
        count += 1;
    }
    assert_eq!(seen, units, "golden must use valid Unicode boundaries");
    count
}
#[test]
fn duplication_matches_original_java_selection_direction_and_mixed_line_endings() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sql-duplicate/original.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 51);
    for case in cases.as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let at = |key| scalar(text, case[key].as_u64().unwrap() as usize);
        let actual =
            tomato_core::sql_edit::duplicate(text, at("start"), at("end"), at("caret")).unwrap();
        let expected = case["result"].as_str().unwrap();
        assert_eq!(actual.text, expected, "{text:?}");
        assert_eq!(
            actual.anchor,
            scalar(expected, case["anchor"].as_u64().unwrap() as usize),
            "{text:?}"
        );
        assert_eq!(
            actual.caret,
            scalar(expected, case["cursor"].as_u64().unwrap() as usize),
            "{text:?}"
        );
    }
}
#[test]
fn effective_statement_uses_selected_nonempty_sql_and_java_trim() {
    use tomato_core::sql_edit::effective;
    let sql = " SELECT 1;\n SELECT 2; ";
    assert_eq!(effective(sql, 12, 21), "SELECT 2;");
    assert_eq!(effective(sql, 21, 12), "SELECT 2;");
    assert_eq!(effective(sql, 0, 1), "SELECT 1;\n SELECT 2;");
    assert_eq!(effective("\u{0}\t select 1 \r\n", 0, 0), "select 1");
    assert_eq!(
        effective("\u{a0}select 1\u{a0}", 0, 0),
        "\u{a0}select 1\u{a0}"
    );
}
