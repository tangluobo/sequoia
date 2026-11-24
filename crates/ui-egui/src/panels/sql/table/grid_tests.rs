#[cfg(test)]
mod menu_metrics_tests {
    use super::super as grid;
    use tomato_core::workbench::*;
    #[test]
    fn actual_table_menu_has_compact_equal_rows() {
        let definition:TableDefinition=serde_json::from_value(serde_json::json!({"table":{"schema":"","name":"fixture"},"columns":[{"name":"value","data_type":"TEXT","nullable":true,"default":null,"primary_key":1,"generated":false,"auto_increment":false,"binary":false,"type_schema":"","type_name":"TEXT"}],"view":false,"transactional":true})).unwrap();
        let mut state = crate::tabs::workbench::Workbench {
            page: Some(TablePage {
                definition,
                rows: vec![vec![Cell::Text("original".into())]],
                total: 1,
                offset: 0,
                limit: 100,
                truncated: false,
                query: Default::default(),
                sql: String::new(),
            }),
            ..Default::default()
        };
        crate::compact_menu::assert_rows_at_height(
            |ui| grid::grid(ui, &mut state, true, "SQLITE"),
            egui::pos2(45., 45.),
            &["设置为空字符串", "设置为 NULL"],
            20.,
        );
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::super::*;
    #[test]
    fn selector_borders_do_not_double_between_header_and_rows_or_after_scrolling() {
        use crate::app::tests::header_divider::solid_pixel;
        for dpi in [1., 1.25, 1.5, 2.] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(dpi);
            let mut state = state();
            for scrolled in [false, true] {
                let events = if scrolled {
                    vec![
                        egui::Event::PointerMoved(egui::pos2(80., 120.)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            phase: egui::TouchPhase::Move,
                            delta: egui::vec2(-21.25, -43.75),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![]
                };
                frame(&ctx, &mut state, events);
                let output = frame(&ctx, &mut state, vec![]);
                let row_edges = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::LineSegment { points, stroke } = &shape.shape
                            && stroke.color == LINE
                            && points[0].y == points[1].y
                            && (points[1].x - points[0].x - SELECTOR).abs() < 0.01
                            && points[0].y > 29. + 3.
                            && points[0].y < 350.
                        {
                            Some(points[0].y)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                assert!(row_edges.len() > 5);
                let primitives = ctx.tessellate(output.shapes, ctx.pixels_per_point());
                let header_top = solid_pixel(&primitives, egui::pos2(5.25, 1.75));
                assert!(
                    header_top.iter().all(|v| (v - 245.).abs() < 1.1),
                    "DPI {dpi}: extra top border {header_top:?}"
                );
                for edge in row_edges {
                    let below = solid_pixel(&primitives, egui::pos2(5.25, edge + 1.25));
                    assert!(
                        below.iter().all(|v| (v - 255.).abs() < 1.1),
                        "DPI {dpi}, scrolled {scrolled}, row {edge}: doubled edge {below:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn configured_font_keeps_row_metrics_and_selection_hit_testing_in_sync() {
        for size in [8, 18, 24] {
            let ctx = egui::Context::default();
            crate::table_style::sync(
                &ctx,
                tomato_core::preferences::TableFont {
                    name: "Sans Serif".into(),
                    size,
                },
                true,
            );
            let mut state = state();
            frame(&ctx, &mut state, vec![]);
            let out = frame(&ctx, &mut state, vec![]);
            let row_height = f32::from(size) + 18.;
            let value = out
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == "row 0" => Some(t),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                value.galley.job.sections[0].format.font_id.size,
                f32::from(size)
            );
            let point = egui::pos2(100., (row_height + 1.) + row_height * 2.5);
            click(&ctx, &mut state, point, egui::Modifiers::NONE);
            assert_eq!(state.table_view.focus.as_ref().unwrap().0, 2, "size {size}");
        }
    }
    fn state() -> Workbench {
        let definition: TableDefinition = serde_json::from_value(serde_json::json!({
            "table":{"schema":"","name":"items"},"columns":(["id","name","note"].map(|name|serde_json::json!({
                "name":name,"data_type":"TEXT","nullable":true,"default":null,"primary_key":0,
                "generated":false,"auto_increment":false,"binary":false,"type_schema":"","type_name":"TEXT"}))),
            "view":false,"transactional":true})).unwrap();
        Workbench {
            definition: Some(definition.clone()),
            page: Some(TablePage {
                definition,
                rows: (0..1000)
                    .map(|i| vec![Cell::Integer(i), Cell::Text(format!("row {i}")), Cell::Null])
                    .collect(),
                total: 2000,
                offset: 0,
                limit: 1000,
                truncated: false,
                query: Default::default(),
                sql: String::new(),
            }),
            ..Default::default()
        }
    }
    fn frame(
        ctx: &egui::Context,
        state: &mut Workbench,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let modifiers = events
            .iter()
            .rev()
            .find_map(|event| match event {
                egui::Event::PointerButton { modifiers, .. }
                | egui::Event::Key { modifiers, .. }
                | egui::Event::ModifiersChanged(modifiers) => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let mut events = events;
        if ctx.input(|input| input.modifiers) != modifiers {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 450.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                grid(ui, state, true, "SQLITE");
            },
        );
        output.textures_delta.clear();
        output
    }
    fn click(
        ctx: &egui::Context,
        state: &mut Workbench,
        point: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        frame(
            ctx,
            state,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ],
        );
        frame(
            ctx,
            state,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            }],
        );
    }
    #[test]
    fn actual_cells_select_copy_and_keyboard_use_original_compact_geometry() {
        let ctx = egui::Context::default();
        let mut state = state();
        frame(&ctx, &mut state, vec![]);
        assert_eq!(state.table_view.widths, vec![60., 60., 60.]);
        click(
            &ctx,
            &mut state,
            egui::pos2(95., 42.),
            egui::Modifiers::NONE,
        );
        assert_eq!(state.table_view.focus, Some((0, 1)));
        assert_eq!(state.table_view.cells.len(), 1);
        let copied = frame(&ctx, &mut state, vec![egui::Event::Copy]);
        assert!(
            copied.platform_output.commands.iter().any(
                |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="row 0")
            )
        );
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(state.table_view.focus, Some((1, 1)));
        assert_eq!(state.table_view.cells.len(), 1);
    }
    #[test]
    fn first_press_starts_selection_drag_keeps_its_anchor_and_control_click_toggles_once() {
        let ctx = egui::Context::default();
        let mut state = state();
        frame(&ctx, &mut state, vec![]);
        frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::PointerMoved(egui::pos2(95., 42.)),
                egui::Event::PointerButton {
                    pos: egui::pos2(95., 42.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(
            state.table_view.focus,
            Some((0, 1)),
            "selection must begin on press, not release"
        );
        assert_eq!(state.table_view.anchor, Some((0, 1)));
        assert!(state.editor.is_none());
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(egui::pos2(35., 70.))],
        );
        assert_eq!(state.table_view.focus, Some((1, 0)));
        assert_eq!(state.table_view.cells.len(), 4);
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(35., 70.),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(state.editor.is_none());
        click(
            &ctx,
            &mut state,
            egui::pos2(35., 42.),
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        );
        assert_eq!(
            state.table_view.cells.len(),
            3,
            "release must not toggle the control-click selection again"
        );
        assert!(!state.table_view.cells.contains(&(0, 0)));
        assert!(state.editor.is_none());
        let focus = state.table_view.focus;
        frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::PointerMoved(egui::pos2(35., 8.)),
                egui::Event::PointerButton {
                    pos: egui::pos2(35., 8.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(egui::pos2(155., 42.))],
        );
        assert_eq!(
            state.table_view.focus, focus,
            "a drag from a different control must not become a grid selection"
        );
        state.clear_changes();
        state.table_view.focus = Some((0, 0));
        state.table_view.cells.insert((0, 1));
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, vec![]);
        click(
            &ctx,
            &mut state,
            egui::pos2(35., 42.),
            egui::Modifiers::NONE,
        );
        assert!(
            state.editor.is_none(),
            "a focused but deselected cell must only select on its first click"
        );
    }
    #[test]
    fn selector_selects_fields_and_page_replacement_discards_transient_selection() {
        let ctx = egui::Context::default();
        let mut state = state();
        frame(&ctx, &mut state, vec![]);
        click(&ctx, &mut state, egui::pos2(7., 42.), egui::Modifiers::NONE);
        assert_eq!(state.table_view.cells.len(), 3);
        assert_eq!(state.selected.iter().copied().collect::<Vec<_>>(), vec![0]);
        select(&mut state, (4, 2), true, false, false);
        assert_eq!(state.table_view.cells.len(), 15);
        let page = state.page.clone().unwrap();
        state.receive(DatabaseOutput::Page(page));
        assert!(state.table_view.cells.is_empty());
        assert!(state.table_view.focus.is_none());
    }
    #[test]
    fn large_page_only_emits_text_for_visible_cells() {
        let ctx = egui::Context::default();
        let mut state = state();
        let output = frame(&ctx, &mut state, vec![]);
        let texts = output
            .shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Text(_)))
            .count();
        assert!(texts < 100, "must not paint 1000 hidden rows: {texts}");
    }
    fn key(ctx: &egui::Context, state: &mut Workbench, key: egui::Key) {
        frame(
            ctx,
            state,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    fn start(ctx: &egui::Context, state: &mut Workbench, column: usize) {
        frame(ctx, state, vec![]);
        click(
            ctx,
            state,
            egui::pos2(35. + column as f32 * 60., 42.),
            egui::Modifiers::NONE,
        );
        assert!(state.editor.is_none(), "first click selects, never edits");
        key(ctx, state, egui::Key::F2);
        frame(ctx, state, vec![]);
        assert!(state.editor.is_some());
    }
    #[test]
    fn inline_typing_enter_escape_and_blur_stage_only_actual_changes() {
        let ctx = egui::Context::default();
        let mut state = state();
        start(&ctx, &mut state, 1);
        assert!(!state.dirty(), "entering edit must not mark a row dirty");
        let rect = state.editor.as_ref().unwrap().rect.unwrap();
        assert_eq!(rect.height(), 28.);
        let output = frame(&ctx, &mut state, vec![]);
        let (position, galley) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "row 0" => {
                    Some((text.pos, text.galley.clone()))
                }
                _ => None,
            })
            .expect("the active cell must paint its own input");
        assert!(
            (position.x - (rect.left() + 6.)).abs() <= 1.,
            "input must retain the original four-pixel text inset inside its border"
        );
        assert!(
            (position.y + galley.size().y * 0.5 - rect.center().y).abs() <= 1.,
            "input text must be centered vertically"
        );
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(egui::pos2(155., 42.))],
        );
        assert!(
            state.editor.is_some(),
            "hover must not close or replace the editor"
        );
        frame(&ctx, &mut state, vec![egui::Event::Text("changed".into())]);
        assert_eq!(state.editor.as_ref().unwrap().text, "changed");
        assert!(state.dirty());
        key(&ctx, &mut state, egui::Key::Escape);
        assert!(state.editor.is_none());
        assert!(!state.dirty());
        key(&ctx, &mut state, egui::Key::F2);
        frame(&ctx, &mut state, vec![]);
        frame(&ctx, &mut state, vec![egui::Event::Text("saved".into())]);
        key(&ctx, &mut state, egui::Key::Enter);
        assert!(state.editor.is_none());
        assert_eq!(state.edits[&0][1], Cell::Text("saved".into()));
        assert_eq!(
            state.page.as_ref().unwrap().rows[0][1],
            Cell::Text("row 0".into())
        );
        key(&ctx, &mut state, egui::Key::F2);
        frame(&ctx, &mut state, vec![]);
        frame(&ctx, &mut state, vec![egui::Event::Text("blurred".into())]);
        click(
            &ctx,
            &mut state,
            egui::pos2(155., 42.),
            egui::Modifiers::NONE,
        );
        assert!(state.editor.is_none());
        assert_eq!(state.table_view.focus, Some((0, 2)));
        assert_eq!(state.edits[&0][1], Cell::Text("blurred".into()));
    }
    #[test]
    fn selected_cell_click_edits_in_place_and_release_discards_undo_state() {
        let ctx = egui::Context::default();
        let mut state = state();
        frame(&ctx, &mut state, vec![]);
        click(
            &ctx,
            &mut state,
            egui::pos2(95., 42.),
            egui::Modifiers::NONE,
        );
        assert!(state.editor.is_none());
        click(
            &ctx,
            &mut state,
            egui::pos2(95., 42.),
            egui::Modifiers::NONE,
        );
        frame(&ctx, &mut state, vec![]);
        assert!(state.editor.is_some());
        let focused = ctx.memory(|memory| memory.focused()).unwrap();
        assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focused).is_some());
        state.clear_changes();
        assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focused).is_none());
    }
    #[test]
    fn entering_and_typing_in_one_frame_keeps_text_and_text_drag_does_not_select_cells() {
        let ctx = egui::Context::default();
        let mut state = state();
        frame(&ctx, &mut state, vec![]);
        click(
            &ctx,
            &mut state,
            egui::pos2(95., 42.),
            egui::Modifiers::NONE,
        );
        frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::Key {
                    key: egui::Key::F2,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Text("quick".into()),
            ],
        );
        assert_eq!(
            state.editor.as_ref().unwrap().text,
            "quick",
            "the first keystroke must not be lost while entering the cell"
        );
        frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::PointerMoved(egui::pos2(95., 42.)),
                egui::Event::PointerButton {
                    pos: egui::pos2(95., 42.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(egui::pos2(165., 42.))],
        );
        assert!(state.editor.is_some());
        assert_eq!(state.table_view.focus, Some((0, 1)));
        assert_eq!(state.table_view.cells.len(), 1);
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(165., 42.),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        key(&ctx, &mut state, egui::Key::Enter);
        assert!(state.editor.is_none());
        assert_eq!(state.edits[&0][1], Cell::Text("quick".into()));
        key(&ctx, &mut state, egui::Key::Enter);
        assert!(
            state.editor.is_some(),
            "Enter used to start editing must not immediately submit the new input"
        );
    }
    #[test]
    fn actual_table_save_button_accepts_the_active_input_with_one_click() {
        use crate::{
            panels::Intent,
            tabs::{Panel, Tab},
        };
        fn show(
            ctx: &egui::Context,
            tab: &mut Tab,
            events: Vec<egui::Event>,
        ) -> (egui::FullOutput, Option<Intent>) {
            let mut intent = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 450.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    intent = super::super::super::render(ui, tab);
                },
            );
            output.textures_delta.clear();
            (output, intent)
        }
        fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        }
        let ctx = egui::Context::default();
        let mut tab =
            crate::tabs::create_tab(tomato_core::Connection::new("SQLITE"), None).unwrap();
        let mut workbench = state();
        workbench.table = Some(workbench.definition.as_ref().unwrap().table.clone());
        if let Panel::Sql {
            workbench: target, ..
        } = &mut tab.panel
        {
            **target = workbench;
        } else {
            panic!("SQL fixture");
        }
        let (output, _) = show(&ctx, &mut tab, vec![]);
        let cell = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "row 0" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        show(&ctx, &mut tab, pointer(cell, true));
        show(&ctx, &mut tab, pointer(cell, false));
        let (output, _) = show(
            &ctx,
            &mut tab,
            vec![
                egui::Event::Key {
                    key: egui::Key::F2,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Text("save once".into()),
            ],
        );
        let save = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Path(path)
                    if path.points.len() == 3
                        && (path.points[1] - path.points[0] - egui::vec2(4., 4.)).length()
                            < 0.01
                        && (path.points[2] - path.points[1] - egui::vec2(7., -7.)).length()
                            < 0.01 =>
                {
                    Some(path.points[0] + egui::vec2(5., 0.))
                }
                _ => None,
            })
            .expect("actual save checkmark");
        show(&ctx, &mut tab, pointer(save, true));
        let (_, intent) = show(&ctx, &mut tab, pointer(save, false));
        let Some(Intent::Command(command)) = intent else {
            panic!("a single save click must emit the apply command");
        };
        let Command::Database {
            action: DatabaseAction::Apply { changes, .. },
            ..
        } = *command
        else {
            panic!("table apply");
        };
        assert_eq!(changes.len(), 1);
        assert!(
            matches!(&changes[0],RowChange::Update {original,values} if original[1]==Cell::Text("row 0".into()) && values[1]==Cell::Text("save once".into()))
        );
    }
    #[test]
    fn null_empty_binary_errors_and_calendar_keep_cell_semantics() {
        let ctx = egui::Context::default();
        let mut state = state();
        start(&ctx, &mut state, 2);
        assert!(state.editor.as_ref().unwrap().text.is_empty());
        key(&ctx, &mut state, egui::Key::Enter);
        assert!(state.edits.is_empty());
        state.page.as_mut().unwrap().rows[0][2] = Cell::Text("NULL".into());
        key(&ctx, &mut state, egui::Key::F2);
        frame(&ctx, &mut state, vec![]);
        key(&ctx, &mut state, egui::Key::Backspace);
        key(&ctx, &mut state, egui::Key::Enter);
        assert_eq!(state.edits[&0][2], Cell::Text(String::new()));
        state.clear_changes();
        state.page.as_mut().unwrap().rows[0][1] = Cell::Binary(vec![1, 2, 3]);
        state.page.as_mut().unwrap().definition.columns[1].binary = true;
        start(&ctx, &mut state, 1);
        frame(&ctx, &mut state, vec![egui::Event::Text("汉字".into())]);
        key(&ctx, &mut state, egui::Key::Enter);
        assert!(state.editor.is_some());
        assert!(state.edits.is_empty());
        assert!(state.status.contains("输入无效"));
        key(&ctx, &mut state, egui::Key::Escape);
        state.page.as_mut().unwrap().rows[0][1] = Cell::Text("2026-10-20".into());
        state.page.as_mut().unwrap().definition.columns[1].binary = false;
        state.page.as_mut().unwrap().definition.columns[1].data_type = "DATE".into();
        key(&ctx, &mut state, egui::Key::F2);
        frame(&ctx, &mut state, vec![]);
        assert!(
            state.editor.as_ref().unwrap().temporal.is_none(),
            "editing a date must not open a popup"
        );
        click(
            &ctx,
            &mut state,
            egui::pos2(123., 42.),
            egui::Modifiers::NONE,
        );
        assert!(
            state.editor.as_ref().unwrap().temporal.is_some(),
            "only the calendar button opens the picker"
        );
    }
}
