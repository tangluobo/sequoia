use super::*;
use tomato_core::{
    backup::{Kind, Selection},
    workbench::TableId,
};
#[test]
fn checkbox_listeners_match_original_java_traces_including_partial_and_runtime_selection() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/backup/original-selection.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut groups = selection::groups(
            ["users", "orders"]
                .map(|name| Selection {
                    kind: Kind::Table,
                    table: TableId {
                        schema: "sample_db".into(),
                        name: name.into(),
                    },
                })
                .into(),
        );
        for (operation, expected) in case["operations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["trace"].as_array().unwrap())
        {
            let operation = operation.as_str().unwrap();
            let value = operation.ends_with('+');
            if operation.starts_with("all") {
                for group in &mut groups {
                    group.set_all(value);
                    if !value {
                        group.set_custom(false);
                    }
                }
            } else if operation.starts_with("custom") {
                groups[0].set_custom(value);
            } else {
                groups[0].set_item(usize::from(operation.as_bytes()[5] - b'0'), value);
            }
            let group = &groups[0];
            let actual = serde_json::json!({"all":group.all,"custom":group.custom,"indeterminate":group.indeterminate(),"children":group.items.iter().map(|item|item.selected).collect::<Vec<_>>(),"selected":group.selected().map(|item|item.table.name.as_str()).collect::<Vec<_>>()});
            assert_eq!(actual, *expected, "operation {operation}, case {case}");
        }
    }
}
#[test]
fn native_backup_controls_select_objects_and_close_cancels_the_running_actor() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let connection = Connection::new("MYSQL");
    let scope = tomato_core::backup::Scope {
        connection: connection.name.clone(),
        database: "sample_db".into(),
        path: String::new(),
    };
    let mut draft = Draft::new(connection, scope, &ctx);
    draft.loading = false;
    draft.tab = 1;
    draft.groups = selection::groups(vec![Selection {
        kind: Kind::Table,
        table: TableId {
            schema: "sample_db".into(),
            name: "users".into(),
        },
    }]);
    let mut icons = crate::icons::Icons::default();
    let mut frame = |events| {
        let mut result = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(720., 580.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                result = view::render(ui, &mut draft, &mut icons);
            },
        );
        output.textures_delta.clear();
        result
    };
    frame(Vec::new());
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(300., 516.)),
            egui::Event::PointerButton {
                pos: egui::pos2(300., 516.),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    assert_eq!(draft.objects().len(), 1);
    draft.running = true;
    let control = draft.control.clone();
    draft.release(&ctx);
    assert!(control.cancelled());
}

#[test]
fn restore_source_caps_long_archive_labels_and_selects_the_exact_name() {
    use crate::app::backup_tree::{Cache, Key};

    let ctx = egui::Context::default();
    let connection = Connection::new("MYSQL");
    let scope = tomato_core::backup::Scope {
        connection: connection.name.clone(),
        database: "sample_db".into(),
        path: "历史备份".into(),
    };
    let mut draft = Draft::new(connection, scope.clone(), &ctx);
    let name = format!("{}-归档", "Production backup 2026 Q4 - ".repeat(7));
    let caption = format!("{name}.nb3");
    let key = Key::new(&draft.connection.id, &scope.database, &scope.path);
    let mut cache = Cache::default();
    let generation = cache.begin(key.clone(), scope);
    cache.finish(
        &key,
        generation,
        Ok(vec![tomato_core::backup::Item {
            name: name.clone(),
            directory: false,
        }]),
    );
    let anchor = egui::Rect::from_min_size(egui::pos2(130., 75.), egui::vec2(370., 24.));
    let mut frame = |events| {
        let mut choice = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                choice = restore_source::render(ui, anchor, &draft, cache.directories.get(&key));
            },
        );
        output.textures_delta.clear();
        if let Some(restore_source::Choice::Archive(name)) = &choice {
            draft.restore = Some(name.clone());
        }
        (output, choice)
    };
    let click = |pos, pressed| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    };
    frame(vec![]);
    for pressed in [true, false] {
        frame(click(anchor.center(), pressed));
    }
    let archive_pos = |output: &egui::FullOutput, caption: &str, width: f32| {
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == caption => Some(text),
                _ => None,
            })
            .expect("archive text missing");
        assert!(text.galley.elided && text.galley.size().x < width);
        text.pos + text.galley.size() / 2.
    };
    let row = archive_pos(&frame(vec![]).0, &caption, anchor.width());
    assert!(row.y > anchor.bottom());
    frame(click(row, true));
    let (_, chosen) = frame(click(row, false));
    assert!(matches!(chosen, Some(restore_source::Choice::Archive(file)) if file == name));
    let shown = archive_pos(&frame(vec![]).0, &name, anchor.width());
    assert!(anchor.contains(shown));
}
