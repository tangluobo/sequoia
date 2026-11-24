use super::script::*;
#[test]
fn sql_script_information_matches_the_original_java_textarea() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sql-types/information.json"
    ))
    .unwrap();
    for case in golden.as_array().unwrap() {
        let mut script = Script {
            elapsed_ms: case["time"].as_u64().unwrap(),
            ..Default::default()
        };
        for item in case["statements"].as_array().unwrap() {
            let start = script.source.len();
            script.source.push_str(item["sql"].as_str().unwrap());
            let end = script.source.len();
            script.source.push_str(";\n");
            let success = item["success"].as_bool().unwrap();
            if success {
                script.success_count += 1;
            } else {
                script.fail_count += 1;
            }
            script.statements.push(Statement {
                range: start..end,
                select: false,
                source_table: None,
                elapsed_ms: item["time"].as_u64().unwrap(),
                success,
                error: Some(item["error"].as_str().unwrap().into()),
                result: None,
                has_result_set: item["rowset"].as_bool().unwrap(),
                row_count: item["rows"].as_u64().unwrap(),
                affected: item["affected"].as_u64(),
                truncated: false,
                editing: None,
                edit_error: None,
            });
        }
        assert_eq!(script.info(), case["text"].as_str().unwrap());
        assert_eq!(case["wrap"], true);
        assert_eq!(case["editable"], false);
    }
}
