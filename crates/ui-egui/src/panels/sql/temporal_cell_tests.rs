use super::results_tests::{fixture, frame, key, pointer};
use crate::tabs::query_results::{Cell, State, editing::Editing, temporal_model::Kind};
#[test]
fn result_jdbc_temporal_columns_open_the_picker_from_the_actual_cell_button() {
    for (code, text, kind) in [
        (91, "2024-02-29", Kind::Date),
        (92, "12:34:56.75", Kind::Time),
        (93, "2024-02-29 12:34:56.75", Kind::DateTime),
        (2013, "12:34:56", Kind::Time),
        (2014, "2024-02-29 12:34:56", Kind::DateTime),
    ] {
        let ctx = egui::Context::default();
        let mut result = fixture();
        result.rows[0][3] = text.into();
        result.column_types = vec![12, 12, 12, code];
        let mut state = State::default();
        state.initialize(&result);
        state.editing = Some(Box::new(Editing::new(
            tomato_services::database::query_rows::Editing {
                table: tomato_core::workbench::TableId {
                    schema: "public".into(),
                    name: "items".into(),
                },
                plan: tomato_core::query_edit::Plan {
                    database_column_names: result.columns.clone(),
                    editable_column_indexes: vec![3],
                    primary_key_columns: vec!["中文字段".into()],
                    uses_original_row_match: false,
                    status_text: "可编辑".into(),
                },
                comments: Default::default(),
            },
        )));
        state.select(Cell { row: 0, column: 3 }, false, false, false);
        state.end_drag();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("result-fixture")));
        frame(&ctx, &result, &mut state, 0., vec![]);
        frame(
            &ctx,
            &result,
            &mut state,
            0.1,
            vec![key(egui::Key::F2, false, false)],
        );
        assert!(
            state
                .editing
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .temporal
                .is_none()
        );
        let right = 16. + state.widths.iter().sum::<f32>();
        let button = egui::pos2(right - 13., 42.);
        for (index, pressed) in [true, false].into_iter().enumerate() {
            frame(
                &ctx,
                &result,
                &mut state,
                0.2 + index as f64 * 0.1,
                pointer(button, pressed, false),
            );
        }
        let editor = state.editing.as_ref().unwrap().editor.as_ref().unwrap();
        let picker = editor
            .temporal
            .as_ref()
            .expect("calendar button must retain the active edit and open its actual picker");
        assert_eq!(picker.value.kind, kind);
        if kind != Kind::Time {
            assert_eq!(picker.value.selected.to_string(), "2024-02-29");
        }
        if kind != Kind::Date {
            assert_eq!(picker.value.time, [12, 34, 56]);
        }
        assert_eq!(editor.text, text);
        assert!(state.error.is_empty());
        assert!(state.editing.as_ref().unwrap().cells.is_empty());
    }
}
