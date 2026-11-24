use tomato_core::sql_completion::{self, Metadata};
fn scalar(text: &str, units: usize) -> usize {
    let mut seen = 0;
    let mut count = 0;
    for ch in text.chars() {
        if seen >= units {
            break;
        }
        seen += ch.len_utf16();
        count += 1;
    }
    assert_eq!(
        seen, units,
        "Golden cursor must be a valid Unicode boundary"
    );
    count
}
#[test]
fn completion_matches_real_java_qualified_unicode_sorted_and_limited_candidates() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sql-completion/java-golden.json")).unwrap();
    let metadata: Metadata = serde_json::from_value(fixture["metadata"].clone()).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let sql = case["sql"].as_str().unwrap();
        let result = sql_completion::complete(
            sql,
            scalar(sql, case["caret"].as_u64().unwrap() as usize),
            &metadata,
        );
        assert_eq!(
            result.start,
            scalar(sql, case["start"].as_u64().unwrap() as usize),
            "{sql}"
        );
        let actual = result
            .candidates
            .iter()
            .map(|candidate| (candidate.text.as_str(), candidate.kind.label()))
            .collect::<Vec<_>>();
        let expected = case["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|candidate| {
                (
                    candidate["text"].as_str().unwrap(),
                    candidate["kind"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{sql}");
    }
}
