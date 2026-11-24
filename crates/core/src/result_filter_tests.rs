use super::*;
#[test]
fn result_filters_match_original_java_decimal_unicode_null_and_operator_rules() -> anyhow::Result<()>
{
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/sql-result/original-filters.json"
    ))?;
    let zeroes: Vec<u32> = serde_json::from_value(golden["digitZeroes"].clone())?;
    assert_eq!(zeroes, ZEROES);
    for case in golden["cases"].as_array().unwrap() {
        let actual = case["actual"].as_str();
        let expected = case["expected"].as_str();
        assert_eq!(
            compare(actual, expected),
            case["comparison"].as_i64().unwrap().cmp(&0),
            "{actual:?} / {expected:?}"
        );
        for (name, operator) in [
            ("EQUAL", Operator::Equal),
            ("NOT_EQUAL", Operator::NotEqual),
            ("LIKE", Operator::Like),
            ("NOT_LIKE", Operator::NotLike),
            ("LESS", Operator::Less),
            ("GREATER", Operator::Greater),
        ] {
            let condition = Condition {
                operator,
                expected: expected.map(str::to_owned),
            };
            assert_eq!(
                condition.matches(actual),
                case["matches"][name].as_bool().unwrap(),
                "{name}: {actual:?} / {expected:?}"
            );
        }
    }
    for case in golden["parsed"].as_array().unwrap() {
        let condition = Condition::parse(case["input"].as_str());
        let name = match condition.operator {
            Operator::Equal => "EQUAL",
            Operator::NotEqual => "NOT_EQUAL",
            Operator::Like => "LIKE",
            Operator::NotLike => "NOT_LIKE",
            Operator::Less => "LESS",
            Operator::Greater => "GREATER",
        };
        assert_eq!(name, case["operator"].as_str().unwrap());
        assert_eq!(condition.expected.as_deref(), case["value"].as_str());
    }
    Ok(())
}
