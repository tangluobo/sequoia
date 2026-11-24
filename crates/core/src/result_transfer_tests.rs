use super::*;
use std::collections::BTreeSet;
#[test]
fn result_transfer_matches_original_java_json_tsv_comments_camel_case_and_reordered_columns()
-> Result<()> {
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/sql-result/original-transfer.json"
    ))?;
    let data: Vec<Vec<Option<String>>> = serde_json::from_value(source["rows"].clone())?;
    let comments: BTreeMap<String, String> = serde_json::from_value(source["comments"].clone())?;
    for case in source["cases"].as_array().unwrap() {
        let columns: Vec<String> = serde_json::from_value(case["columns"].clone())?;
        let identities: Vec<usize> = serde_json::from_value(case["identities"].clone())?;
        let selected: Vec<(usize, usize)> = serde_json::from_value(case["selected"].clone())?;
        let rows: Vec<_> = selected
            .iter()
            .map(|(row, _)| *row)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let selected_columns: Vec<_> = selected
            .iter()
            .map(|(_, column)| *column)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for (field, kind) in [
            ("json", Format::Json { camel: false }),
            ("camelJson", Format::Json { camel: true }),
            ("comments", Format::Comments),
            (
                "data",
                Format::Tsv {
                    headers: false,
                    data: true,
                },
            ),
            (
                "headers",
                Format::Tsv {
                    headers: true,
                    data: false,
                },
            ),
            (
                "headersData",
                Format::Tsv {
                    headers: true,
                    data: true,
                },
            ),
        ] {
            let actual = format(
                Area {
                    columns: &columns,
                    rows: &rows,
                    selected_columns: &selected_columns,
                    value: |row: usize, column: usize| data[row][identities[column]].as_deref(),
                },
                kind,
                &comments,
            )?;
            let expected = case[field].as_str().unwrap();
            let expected = if !cfg!(windows) && !matches!(kind, Format::Json { .. }) {
                expected.replace("\r\n", "\n")
            } else {
                expected.into()
            };
            assert_eq!(actual, expected, "sample {} / {field}", case["sample"]);
        }
        let rows: Vec<_> = (0..data.len()).collect();
        let selected_columns: Vec<_> = (0..columns.len()).collect();
        let actual = format(
            Area {
                columns: &columns,
                rows: &rows,
                selected_columns: &selected_columns,
                value: |row: usize, column: usize| data[row][identities[column]].as_deref(),
            },
            Format::Tsv {
                headers: true,
                data: true,
            },
            &comments,
        )?;
        assert_eq!(
            actual,
            case["all"].as_str().unwrap().replace("\r\n", LINE_ENDING)
        );
    }
    for case in source["camel"].as_array().unwrap() {
        assert_eq!(
            camel(case["input"].as_str()),
            case["output"].as_str().unwrap(),
            "{}",
            case["input"]
        );
    }
    Ok(())
}
#[test]
fn result_transfer_preserves_original_copy_sql_quoting_without_executing_it() -> Result<()> {
    let columns = vec!["id".into(), "中文\"字段".into()];
    let rows = [0usize];
    let indexes = [0usize, 1];
    let data = [Some("1"), Some("a'b\n中文")];
    let comments = BTreeMap::new();
    let area = || Area {
        columns: &columns,
        rows: &rows,
        selected_columns: &indexes,
        value: |_: usize, column: usize| data[column],
    };
    assert_eq!(
        format(
            area(),
            Format::Insert {
                table: "public.中文表"
            },
            &comments
        )?,
        "INSERT INTO public.\"中文表\" (id, \"中文\"\"字段\") VALUES ('1', 'a''b\n中文');"
    );
    let keys = vec!["id".into()];
    assert_eq!(
        format(
            area(),
            Format::Update {
                table: "items",
                primary_keys: &keys
            },
            &comments
        )?,
        "UPDATE items SET \"中文\"\"字段\" = 'a''b\n中文' WHERE id = '1';"
    );
    // The original query menu disables Update when its PK supplier is empty.
    // Keep the unreachable helper's text behavior separate from menu eligibility;
    // this formatter never executes SQL.
    assert_eq!(
        format(
            area(),
            Format::Update {
                table: "items",
                primary_keys: &[]
            },
            &comments
        )?,
        "UPDATE items SET id = '1', \"中文\"\"字段\" = 'a''b\n中文' WHERE ;"
    );
    Ok(())
}
