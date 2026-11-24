use super::*;
use tomato_core::{
    database_objects::{Field, ForeignKey, Object},
    workbench::TableId,
};
fn fixture() -> Catalog {
    let table = |name: &str| TableId {
        schema: "sample_db".into(),
        name: name.into(),
    };
    Catalog {
        objects: [
            ("orders", "订单", false),
            ("users", "用户", false),
            ("active_users", "用户视图", true),
        ]
        .into_iter()
        .map(|(name, comment, view)| Object {
            table: table(name),
            comment: comment.into(),
            view,
            engine: String::new(),
            auto_increment: String::new(),
            update_time: String::new(),
            data_length: String::new(),
            rows: String::new(),
            fields: Vec::new(),
        })
        .collect(),
        foreign_keys: vec![ForeignKey {
            name: "fk_user".into(),
            table: table("orders"),
            columns: vec!["user_id".into()],
            referenced: table("users"),
            referenced_columns: vec!["id".into()],
        }],
    }
}
#[test]
fn selection_filter_anchor_and_status_match_actual_java_object_view() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/objects/original-selection.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut state = State::default();
        state.receive(fixture());
        for (action, expected) in case["actions"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["states"].as_array().unwrap())
        {
            let action = action.as_str().unwrap();
            if action == "all" {
                state.select_all();
            } else if let Some(query) = action.strip_prefix("search:") {
                state.search = query.into();
                state.filter();
            } else {
                let (kind, index) = action.split_once(':').unwrap();
                state.click(
                    index.parse().unwrap(),
                    matches!(kind, "ctrl" | "ctrlshift"),
                    matches!(kind, "shift" | "ctrlshift"),
                );
            }
            let names = |indices: &[usize]| {
                indices
                    .iter()
                    .map(|index| state.catalog.objects[*index].table.name.as_str())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                serde_json::to_value(names(&state.selection_order)).unwrap(),
                expected["selected"],
                "selected after {action}"
            );
            assert_eq!(
                serde_json::to_value(names(&state.filtered)).unwrap(),
                expected["visible"],
                "filtered after {action}"
            );
            assert_eq!(
                state
                    .primary
                    .map(|index| state.catalog.objects[index].table.name.as_str()),
                expected["selectedObject"].as_str(),
                "primary after {action}"
            );
            assert_eq!(
                state
                    .anchor
                    .map(|index| state.catalog.objects[index].table.name.as_str()),
                expected["anchorObject"].as_str(),
                "anchor after {action}"
            );
            assert_eq!(
                state.status(),
                expected["status"].as_str().unwrap(),
                "status after {action}"
            );
        }
    }
}
#[test]
fn field_search_and_copy_keep_real_names_views_and_click_order_without_clipboard_writes() {
    let mut catalog = fixture();
    catalog.objects[0].fields.push(Field {
        name: "supplier_id".into(),
        comment: "关联供应商".into(),
    });
    let mut state = State::default();
    state.receive(catalog);
    state.search = "  供应商  ".into();
    state.filter();
    assert_eq!(state.filtered, [0]);
    state.search = "SUPPLIER".into();
    state.filter();
    assert_eq!(state.filtered, [0]);
    state.search.clear();
    state.filter();
    state.click(2, false, false);
    state.click(0, true, false);
    assert_eq!(
        state.copy_tables("fixture-id", "sample_db", ""),
        Some("TOMATO_COPY_TABLES\nfixture-id\nsample_db\n\nactive_users\norders".into())
    );
    state.click(2, true, false);
    assert_eq!(state.primary, Some(0));
    assert_eq!(state.anchor, Some(0));
}
#[test]
fn er_diagram_filters_views_and_outside_namespaces_and_keeps_only_indices() {
    let mut catalog = fixture();
    let mut outside = catalog.foreign_keys[0].clone();
    outside.referenced.schema = "another_schema".into();
    catalog.foreign_keys.push(outside);
    let layout = Diagram::new(&catalog, &[0, 1, 2]);
    assert_eq!(layout.tables, [0, 1]);
    assert_eq!(layout.links, [(0, 1, 0)]);
    assert_eq!(layout.columns, 2);
    assert_eq!(
        layout.rect(egui::Pos2::ZERO, 1),
        egui::Rect::from_min_size(egui::pos2(300., 20.), egui::vec2(182., 25.))
    );
    let filtered = Diagram::new(&catalog, &[1, 2]);
    assert_eq!(filtered.tables, [1]);
    assert!(filtered.links.is_empty());
    let mut state = State::default();
    state.receive(catalog);
    let first = state.diagram() as *const Diagram;
    assert_eq!(
        first,
        state.diagram() as *const Diagram,
        "ordinary frames must reuse the ER projection"
    );
    state.search = "users".into();
    state.filter();
    assert!(
        state.diagram().links.is_empty(),
        "filtering must invalidate links to hidden tables"
    );
}
