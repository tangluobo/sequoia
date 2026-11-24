use super::*;
use tomato_core::result_filter::{Condition, Operator};
#[test]
fn combined_result_projection_matches_actual_java_sort_filter_delete_and_column_transitions()
-> anyhow::Result<()> {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/tests/fixtures/sql-result/original-view.json"
    ))?;
    let mut result = QueryResult {
        columns: vec!["value".into(), "amount".into(), "identity".into()],
        ..Default::default()
    };
    for (row, values) in golden[0]["rows"].as_array().unwrap().iter().enumerate() {
        result.rows.push(
            values
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(column, value)| {
                    if value.is_null() {
                        let index = row * 3 + column;
                        result.null_cells.resize(index / 64 + 1, 0);
                        result.null_cells[index / 64] |= 1 << (index % 64);
                    }
                    value.as_str().unwrap_or_default().to_owned()
                })
                .collect(),
        );
    }
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(editing::Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan: tomato_core::query_edit::Plan {
                database_column_names: result.columns.clone(),
                editable_column_indexes: vec![0, 1, 2],
                primary_key_columns: vec!["identity".into()],
                uses_original_row_match: false,
                status_text: "可编辑".into(),
            },
            comments: Default::default(),
        },
    )));
    for case in golden.as_array().unwrap() {
        match case["name"].as_str().unwrap() {
            "initial" => {}
            "selected-r0" => {
                state.select(Cell { row: 0, column: 0 }, false, false, false);
                state.end_drag();
            }
            "value-ascending" => state.set_sort(&result, 0, false, false),
            "like-b" => state.set_filter(
                &result,
                0,
                Condition {
                    operator: Operator::Like,
                    expected: Some("b".into()),
                },
            )?,
            "amount-descending" => state.set_sort(&result, 1, true, false),
            "and-less-five" => state.set_filter(
                &result,
                1,
                Condition {
                    operator: Operator::Less,
                    expected: Some("5".into()),
                },
            )?,
            "remove-value-filter" => state.remove_filter(&result, 0),
            "clear-sort-filtered" => state.remove_sort(&result, None),
            "clear-all-filters" => state.clear_filters(),
            "amount-ascending" => state.set_sort(&result, 1, false, false),
            "equals-null" => state.set_filter(
                &result,
                0,
                Condition {
                    operator: Operator::Equal,
                    expected: None,
                },
            )?,
            "delete-visible-null-row" => {
                let row = state.row(0);
                state.editing.as_mut().unwrap().deleted.insert(row);
                state.committed(&mut result, true);
            }
            "clear-after-delete" => {
                state.remove_sort(&result, None);
                state.clear_filters();
            }
            "hide-and-reorder" => {
                state.set_column_visible(0, false);
                state.reorder_columns(vec![2, 1, 0])?;
            }
            "filter-physical-amount-after-reorder" => state.set_filter(
                &result,
                1,
                Condition {
                    operator: Operator::Equal,
                    expected: Some("2".into()),
                },
            )?,
            "sort-active-filtered" => state.set_sort(&result, 1, true, false),
            "clear-filter-with-active-sort" => state.clear_filters(),
            "tie-base" => {
                state.set_column_visible(0, true);
                state.set_sort(&result, 0, false, false);
                state.set_filter(
                    &result,
                    0,
                    Condition {
                        operator: Operator::Like,
                        expected: Some("b".into()),
                    },
                )?;
            }
            "tie-reverse" => state.set_sort(&result, 1, true, false),
            "tie-filtered-back-to-value" => state.set_sort(&result, 0, false, false),
            "all-hidden-row-selected" => {
                state.clear_filters();
                for column in 0..3 {
                    state.set_column_visible(column, false);
                }
                state.select(Cell { row: 0, column: 0 }, false, false, true);
                state.end_drag();
            }
            name => panic!("unhandled original transition {name}"),
        }
        let rows: Vec<Vec<_>> = (0..state.visible_rows())
            .map(|row| {
                (0..3)
                    .map(|column| result.value(state.row(row), column))
                    .collect()
            })
            .collect();
        assert_eq!(
            serde_json::to_value(rows)?,
            case["rows"],
            "{}",
            case["name"]
        );
        let selected: Vec<_> = (0..state.visible_rows())
            .filter(|row| state.row_selected(*row))
            .map(|row| result.value(state.row(row), 2))
            .collect();
        assert_eq!(
            serde_json::to_value(selected)?,
            case["selectedRows"],
            "{}",
            case["name"]
        );
        let sorts:Vec<_>=state.view.sorts.iter().map(|(column,descending)|serde_json::json!({"column":result.columns[*column],"type":if *descending{"DESCENDING"}else{"ASCENDING"}})).collect();
        assert_eq!(
            serde_json::to_value(sorts)?,
            case["sorts"],
            "{}",
            case["name"]
        );
        let columns:Vec<_>=state.view.columns.iter().map(|column|serde_json::json!({"label":result.columns[column.identity],"identity":column.identity,"visible":column.visible})).collect();
        assert_eq!(
            serde_json::to_value(columns)?,
            case["columns"],
            "{}",
            case["name"]
        );
    }
    Ok(())
}
