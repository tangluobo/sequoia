use tomato_core::markdown_edit::{self, Position, Rectangle};
fn scalar(text: &str, units: usize) -> usize {
    let mut count = 0;
    let mut chars = 0;
    for ch in text.chars() {
        if count >= units {
            break;
        }
        count += ch.len_utf16();
        chars += 1;
    }
    assert_eq!(
        count, units,
        "Golden cursor must be a valid Unicode boundary"
    );
    chars
}
#[test]
fn editing_matches_real_original_java_text_and_selection_goldens() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/markdown-edit/java-golden.json")).unwrap();
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let input = case["input"].as_str().unwrap();
        let start = scalar(input, case["start"].as_u64().unwrap() as usize);
        let end = scalar(input, case["end"].as_u64().unwrap() as usize);
        let edit = match case["action"].as_str().unwrap() {
            "indent" => Some(
                markdown_edit::indent(input, start..end, case["shift"].as_bool().unwrap()).unwrap(),
            ),
            "enter" => markdown_edit::continue_line(input, end).unwrap(),
            "rectangle" => {
                let p = case["rectangle"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i.as_u64().unwrap() as usize)
                    .collect::<Vec<_>>();
                Some(
                    Rectangle {
                        start: Position {
                            row: p[0],
                            column: p[1],
                        },
                        end: Position {
                            row: p[2],
                            column: p[3],
                        },
                    }
                    .edit(input, case["replacement"].as_str().unwrap())
                    .unwrap(),
                )
            }
            _ => panic!("unknown golden action"),
        };
        assert_eq!(
            edit.is_some(),
            case["handled"].as_bool().unwrap(),
            "case {index}"
        );
        if let Some(edit) = edit {
            let expected = case["output"].as_str().unwrap();
            assert_eq!(edit.text, expected, "case {index}");
            assert_eq!(
                edit.selection,
                scalar(expected, case["selectionStart"].as_u64().unwrap() as usize)
                    ..scalar(expected, case["selectionEnd"].as_u64().unwrap() as usize),
                "case {index}"
            );
        }
    }
}
