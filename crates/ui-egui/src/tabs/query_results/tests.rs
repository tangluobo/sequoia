use super::*;
#[test]
fn sort_cycle_and_selected_record_match_the_original_java_result_control() -> anyhow::Result<()> {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../core/tests/fixtures/sql-result/original-sort.json"
    ))?;
    let mut result = QueryResult {
        columns: vec!["value".into(), "identity".into()],
        rows: golden[0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap_or_default().into())
                    .collect()
            })
            .collect(),
        ..Default::default()
    };
    for (row, values) in golden[0]["rows"].as_array().unwrap().iter().enumerate() {
        for (column, value) in values.as_array().unwrap().iter().enumerate() {
            if value.is_null() {
                let index = row * 2 + column;
                result.null_cells.resize(index / 64 + 1, 0);
                result.null_cells[index / 64] |= 1 << (index % 64);
            }
        }
    }
    let mut state = State::default();
    state.initialize(&result);
    state.select(Cell { row: 0, column: 0 }, false, false, false);
    state.end_drag();
    for (index, case) in golden.as_array().unwrap().iter().enumerate() {
        if index > 0 {
            state.sort_by(&result, 0);
        }
        let rows: Vec<Vec<_>> = (0..result.rows.len())
            .map(|row| {
                (0..2)
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
        let selected: Vec<_> = (0..result.rows.len())
            .filter(|&row| (0..2).any(|column| state.contains(Cell { row, column })))
            .collect();
        assert_eq!(
            serde_json::to_value(selected)?,
            case["selectedRows"],
            "{}",
            case["name"]
        );
    }
    Ok(())
}
#[test]
fn selections_keep_row_identity_during_sort_and_copy_sparse_cells_without_filling_holes()
-> anyhow::Result<()> {
    let result = QueryResult {
        columns: vec!["name".into(), "literal".into(), "empty".into()],
        rows: vec![
            vec!["b".into(), "NULL".into(), String::new()],
            vec!["a".into(), "v".into(), "tail".into()],
            vec!["c".into(), "last".into(), "end".into()],
        ],
        ..Default::default()
    };
    let mut state = State::default();
    state.initialize(&result);
    assert!(state.selected.is_empty());
    state.select(Cell { row: 0, column: 0 }, false, false, false);
    state.select(Cell { row: 2, column: 2 }, false, true, false);
    assert_eq!(state.copy(&result)?.as_deref(), Some("b\n\nend"));
    state.sort_by(&result, 0);
    assert_eq!(state.row(0), 1);
    assert!(state.contains(Cell { row: 1, column: 0 }));
    assert!(state.contains(Cell { row: 2, column: 2 }));
    assert_eq!(state.copy(&result)?.as_deref(), Some("b\nend"));
    state.select(Cell { row: 0, column: 0 }, false, false, true);
    assert_eq!(state.copy(&result)?.as_deref(), Some("a\tv\ttail"));
    state.select(Cell { row: 0, column: 0 }, false, true, true);
    assert_eq!(state.copy(&result)?, None);
    Ok(())
}
#[test]
fn drag_all_and_java_utf16_sort_use_bounded_bits_instead_of_cell_objects() {
    let result = QueryResult {
        columns: vec!["name".into(); 4],
        rows: (0..10000).map(|i| vec![i.to_string(); 4]).collect(),
        ..Default::default()
    };
    let mut state = State::default();
    state.initialize(&result);
    state.select_all();
    assert_eq!(state.selected.len(), 625);
    assert!(state.selected.capacity() * 8 <= 10000);
    state.select(
        Cell {
            row: 500,
            column: 1,
        },
        false,
        false,
        false,
    );
    state.drag_to(Cell {
        row: 9000,
        column: 3,
    });
    state.end_drag();
    assert!(state.contains(Cell {
        row: 9000,
        column: 2
    }));
    assert!(!state.contains(Cell {
        row: 499,
        column: 2
    }));
    assert!(!state.contains(Cell {
        row: 9000,
        column: 0
    }));
    let result = QueryResult {
        columns: vec!["unicode".into()],
        rows: vec![vec!["\u{e000}".into()], vec!["😀".into()]],
        ..Default::default()
    };
    let mut state = State::default();
    state.initialize(&result);
    state.sort_by(&result, 0);
    assert_eq!(state.row(0), 1);
}
