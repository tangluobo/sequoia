//! Drive complete SQL panels with pointer selection and the Windows key + clipboard event pair.
use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use tomato_core::workbench::{Cell, TableDefinition, TablePage};

pub(super) fn fixture(table: bool, script: bool) -> Tab {
    let mut tab = crate::tabs::create_tab(Connection::new("SQLITE"), None).unwrap();
    let Panel::Sql {
        results, workbench, ..
    } = &mut tab.panel
    else {
        unreachable!()
    };
    let definition: TableDefinition = serde_json::from_value(serde_json::json!({
        "table":{"schema":"main","name":"items"},"view":false,"transactional":true,
        "columns":(["id","name"].map(|name|serde_json::json!({"name":name,"data_type":"TEXT","nullable":true,"default":null,"primary_key":if name=="id"{1}else{0},"generated":false,"auto_increment":false,"binary":false,"type_schema":"","type_name":"TEXT"})))
    })).unwrap();
    if table {
        workbench.table = Some(definition.table.clone());
        workbench.definition = Some(definition.clone());
        workbench.page = Some(TablePage {
            definition,
            rows: vec![
                vec![Cell::Integer(1), Cell::Text("Alice".into())],
                vec![Cell::Integer(2), Cell::Text("Bob".into())],
                vec![Cell::Integer(3), Cell::Text("Charlie".into())],
            ],
            offset: 0,
            limit: 1000,
            total: 3,
            truncated: false,
            query: Default::default(),
            sql: String::new(),
        });
    } else {
        results.push(tomato_services::QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec!["1".into(), "Alice".into()],
                vec!["2".into(), "Bob".into()],
                vec!["3".into(), "Charlie".into()],
            ],
            ..Default::default()
        });
        let mut state = crate::tabs::query_results::State::default();
        state.initialize(&results[0]);
        state.editing = Some(Box::new(crate::tabs::query_results::editing::Editing::new(
            tomato_services::database::query_rows::Editing {
                table: definition.table,
                plan: tomato_core::query_edit::Plan {
                    database_column_names: results[0].columns.clone(),
                    editable_column_indexes: vec![0, 1],
                    primary_key_columns: vec!["id".into()],
                    uses_original_row_match: false,
                    status_text: "可编辑".into(),
                },
                comments: Default::default(),
            },
        )));
        workbench.query_results.push(state);
        if script {
            workbench.script_view = 1;
            workbench.script = Some(Box::new(tomato_services::database::script::Script {
                source: "SELECT * FROM items".into(),
                statements: vec![tomato_services::database::script::Statement {
                    range: 0..19,
                    select: true,
                    source_table: Some("items".into()),
                    elapsed_ms: 0,
                    success: true,
                    error: None,
                    result: Some(0),
                    has_result_set: true,
                    row_count: 3,
                    affected: None,
                    truncated: false,
                    editing: None,
                    edit_error: None,
                }],
                success_count: 1,
                ..Default::default()
            }));
        }
    }
    tab
}

pub(super) fn frame(ctx: &egui::Context, tab: &mut Tab, events: Vec<Event>) -> egui::FullOutput {
    frame_with_intent(ctx, tab, events).0
}
pub(super) fn frame_with_intent(
    ctx: &egui::Context,
    tab: &mut Tab,
    events: Vec<Event>,
) -> (egui::FullOutput, Option<Intent>) {
    let mut icons = crate::icons::Icons::default();
    let mut intent = None;
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 0.1),
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1000., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            intent = crate::panels::render(ui, tab, &mut icons);
        },
    );
    out.textures_delta.clear();
    (out, intent)
}
pub(super) fn click(ctx: &egui::Context, tab: &mut Tab, point: Pos2, modifiers: Modifiers) {
    for pressed in [true, false] {
        frame(
            ctx,
            tab,
            vec![
                Event::ModifiersChanged(modifiers),
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers,
                },
            ],
        );
    }
}
fn row(out: &egui::FullOutput, name: &str) -> Pos2 {
    out.shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(egui::pos2(7., text.pos.y + text.galley.size().y / 2.))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing row {name}"))
}
fn shortcut(key: Key, semantic: Event) -> Vec<Event> {
    let modifiers = Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    vec![
        Event::ModifiersChanged(modifiers),
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        },
        semantic,
    ]
}

#[test]
fn clipboard_shortcuts_follow_pointer_row_selection_in_complete_table_and_query_panels() {
    for selection in 0..3 {
        for (table, script) in [(true, false), (false, false), (false, true)] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut tab = fixture(table, script);
            frame(&ctx, &mut tab, vec![]);
            let out = frame(&ctx, &mut tab, vec![]);
            let first = row(&out, "Alice");
            let second = row(&out, "Bob");
            let third = row(&out, "Charlie");
            click(&ctx, &mut tab, first, Modifiers::default());
            if selection == 2 {
                frame(
                    &ctx,
                    &mut tab,
                    vec![
                        Event::ModifiersChanged(Modifiers::default()),
                        Event::PointerMoved(first),
                        Event::PointerButton {
                            pos: first,
                            button: PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::default(),
                        },
                    ],
                );
                frame(&ctx, &mut tab, vec![Event::PointerMoved(second)]);
                frame(
                    &ctx,
                    &mut tab,
                    vec![Event::PointerButton {
                        pos: second,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::default(),
                    }],
                );
            } else {
                click(
                    &ctx,
                    &mut tab,
                    second,
                    Modifiers {
                        shift: selection == 0,
                        ctrl: selection == 1,
                        command: selection == 1,
                        ..Default::default()
                    },
                );
            }
            // Idle repaint must not drop the focus restored on mouse release.
            frame(&ctx, &mut tab, vec![]);
            let out = frame(&ctx, &mut tab, shortcut(Key::C, Event::Copy));
            let copied = out
                .platform_output
                .commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                copied,
                ["1\tAlice\n2\tBob"],
                "table={table}, script={script}, focus={:?}",
                ctx.memory(|m| m.focused())
            );
            click(
                &ctx,
                &mut tab,
                if table { first } else { second },
                Modifiers::default(),
            );
            frame(
                &ctx,
                &mut tab,
                shortcut(Key::V, Event::Paste(copied[0].clone())),
            );
            let Panel::Sql {
                results, workbench, ..
            } = &tab.panel
            else {
                unreachable!()
            };
            if table {
                assert_eq!(workbench.inserted.len(), 2, "paste must run exactly once");
            } else {
                assert_eq!(
                    workbench.query_results[0].display_value(&results[0], 1, 1),
                    Some("Alice")
                );
                assert_eq!(
                    workbench.query_results[0].display_value(&results[0], 2, 1),
                    Some("Bob")
                );
            }
            // Ensure the coordinate really came from the third data row, not the editor.
            assert!(third.y > second.y);
        }
    }
}

#[test]
fn clipboard_shortcuts_stay_in_the_active_cell_editor_and_sql_text_editor() {
    for (table, script) in [(true, false), (false, false), (false, true)] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tab = fixture(table, script);
        frame(&ctx, &mut tab, vec![]);
        let out = frame(&ctx, &mut tab, vec![]);
        let point = row(&out, "Alice") + egui::vec2(50., 0.);
        click(&ctx, &mut tab, point, Modifiers::default());
        frame(
            &ctx,
            &mut tab,
            vec![Event::Key {
                key: Key::F2,
                physical_key: Some(Key::F2),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }],
        );
        frame(&ctx, &mut tab, vec![]);
        frame(
            &ctx,
            &mut tab,
            shortcut(
                Key::A,
                Event::Key {
                    key: Key::A,
                    physical_key: Some(Key::A),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
            ),
        );
        frame(&ctx, &mut tab, shortcut(Key::V, Event::Paste("99".into())));
        let Panel::Sql { workbench, .. } = &tab.panel else {
            unreachable!()
        };
        assert!(workbench.inserted.is_empty());
        if table {
            assert_eq!(workbench.editor.as_ref().unwrap().text, "99");
        } else {
            assert_eq!(
                workbench.query_results[0]
                    .editing
                    .as_ref()
                    .unwrap()
                    .editor
                    .as_ref()
                    .unwrap()
                    .text,
                "99"
            );
        }
        let out = frame(&ctx, &mut tab, shortcut(Key::C, Event::Copy));
        // The paste collapsed the text selection; copying must not copy selected rows.
        assert!(!out.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text.contains("Alice"))));
        if !table {
            let Panel::Sql { workbench, .. } = &tab.panel else {
                unreachable!()
            };
            let point = workbench.completion.galley_pos + egui::vec2(5., 5.);
            click(&ctx, &mut tab, point, Modifiers::default());
            frame(
                &ctx,
                &mut tab,
                shortcut(
                    Key::A,
                    Event::Key {
                        key: Key::A,
                        physical_key: Some(Key::A),
                        pressed: false,
                        repeat: false,
                        modifiers: Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    },
                ),
            );
            frame(
                &ctx,
                &mut tab,
                shortcut(Key::V, Event::Paste("SELECT 42;".into())),
            );
            let Panel::Sql { sql, workbench, .. } = &tab.panel else {
                unreachable!()
            };
            assert_eq!(sql, "SELECT 42;");
            assert!(
                workbench.query_results[0]
                    .editing
                    .as_ref()
                    .unwrap()
                    .editor
                    .is_none()
            );
        }
    }
}

#[test]
fn clipboard_shortcuts_copy_readonly_results_without_mutating_them() {
    for table in [true, false] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tab = fixture(table, false);
        let Panel::Sql { workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        if table {
            workbench.definition.as_mut().unwrap().view = true;
        } else {
            workbench.query_results[0].editing = None;
        }
        frame(&ctx, &mut tab, vec![]);
        let out = frame(&ctx, &mut tab, vec![]);
        click(&ctx, &mut tab, row(&out, "Alice"), Modifiers::default());
        let out = frame(&ctx, &mut tab, shortcut(Key::C, Event::Copy));
        assert!(out.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="1\tAlice")
        ));
        frame(
            &ctx,
            &mut tab,
            shortcut(Key::V, Event::Paste("9\tchanged".into())),
        );
        let Panel::Sql {
            results, workbench, ..
        } = &tab.panel
        else {
            unreachable!()
        };
        assert!(workbench.inserted.is_empty() && workbench.edits.is_empty());
        if !table {
            assert_eq!(
                workbench.query_results[0].display_value(&results[0], 0, 1),
                Some("Alice")
            );
        }
    }
}
