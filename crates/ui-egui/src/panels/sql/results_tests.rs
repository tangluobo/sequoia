use super::*;
use crate::tabs::query_results::{Cell, State};
pub(super) fn fixture() -> tomato_services::QueryResult {
    let mut result = tomato_services::QueryResult {
        columns: vec![
            "missing".into(),
            "empty".into(),
            "literal".into(),
            "中文字段".into(),
        ],
        rows: (0..2000)
            .map(|row| {
                vec![
                    String::new(),
                    String::new(),
                    "NULL".into(),
                    format!("第 {row} 行"),
                ]
            })
            .collect(),
        null_cells: vec![0; 8000usize.div_ceil(64)],
        ..Default::default()
    };
    for row in 0..2000 {
        let index = row * 4;
        result.null_cells[index / 64] |= 1 << (index % 64);
    }
    result
}
#[test]
fn query_result_f2_types_into_the_real_cell_copies_pending_value_and_releases_undo_state() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let result = fixture();
    let mut state = State::default();
    state.initialize(&result);
    state.editing = Some(Box::new(crate::tabs::query_results::editing::Editing::new(
        tomato_services::database::query_rows::Editing {
            table: tomato_core::workbench::TableId {
                schema: "public".into(),
                name: "items".into(),
            },
            plan: tomato_core::query_edit::Plan {
                database_column_names: result.columns.clone(),
                editable_column_indexes: vec![1],
                primary_key_columns: vec!["empty".into()],
                uses_original_row_match: false,
                status_text: "可编辑".into(),
            },
            comments: Default::default(),
        },
    )));
    state.select(Cell { row: 0, column: 1 }, false, false, false);
    let id = egui::Id::new("result-fixture");
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(&ctx, &result, &mut state, 0.0, vec![]);
    frame(
        &ctx,
        &result,
        &mut state,
        0.1,
        vec![key(egui::Key::F2, false, false)],
    );
    assert!(state.editing.as_ref().unwrap().editor.is_some());
    frame(
        &ctx,
        &result,
        &mut state,
        0.2,
        vec![egui::Event::Text("修改值".into())],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.3,
        vec![key(egui::Key::Enter, false, false)],
    );
    assert_eq!(state.display_value(&result, 0, 1), Some("修改值"));
    assert_eq!(result.value(0, 1), Some(""));
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, id.with("cell-editor")).is_none());
    let copied = frame(&ctx, &result, &mut state, 0.4, vec![egui::Event::Copy]);
    assert_eq!(copied.copied, ["修改值"]);
}
pub(super) struct Frame {
    pub(super) text: Vec<(String, egui::Pos2, Color32)>,
    pub(super) copied: Vec<String>,
}
pub(super) fn frame(
    ctx: &egui::Context,
    result: &tomato_services::QueryResult,
    state: &mut State,
    time: f64,
    events: Vec<egui::Event>,
) -> Frame {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700., 450.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            results::render(ui, result, state, egui::Id::new("result-fixture"));
        },
    );
    let text = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some((
                    text.galley.job.text.clone(),
                    text.pos,
                    text.galley
                        .job
                        .sections
                        .first()
                        .map_or(Color32::BLACK, |section| section.format.color),
                ))
            } else {
                None
            }
        })
        .collect();
    let copied = output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();
    output.textures_delta.clear();
    Frame { text, copied }
}
pub(super) fn key(key: egui::Key, control: bool, shift: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: control,
            command: control,
            shift,
            ..Default::default()
        },
    }
}
pub(super) fn pointer(point: egui::Pos2, pressed: bool, control: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers {
                ctrl: control,
                command: control,
                ..Default::default()
            },
        },
    ]
}
#[test]
fn scrollbar_arrows_page_and_thumb_move_the_actual_virtual_result_rows() {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    let initial = frame(&ctx, &result, &mut state, 0., vec![]);
    let first_y = |frame: &Frame| {
        frame
            .text
            .iter()
            .find(|(text, _, _)| text == "第 0 行")
            .map(|(_, point, _)| point.y)
    };
    let initial_y = first_y(&initial).unwrap();
    let down = egui::pos2(692., 443.);
    frame(&ctx, &result, &mut state, 0.1, pointer(down, true, false));
    let one = frame(&ctx, &result, &mut state, 0.12, pointer(down, false, false));
    assert!(
        (first_y(&one).unwrap() - (initial_y - 10.)).abs() < 0.1,
        "original VirtualScrollBar increment is ten pixels"
    );
    let page = egui::pos2(692., 200.);
    frame(&ctx, &result, &mut state, 0.2, pointer(page, true, false));
    let paged = frame(&ctx, &result, &mut state, 0.22, pointer(page, false, false));
    assert!(!paged.text.iter().any(|(text, _, _)| text == "第 0 行"));
    assert!(paged.text.iter().any(|(text, _, _)| text == "第 15 行"));
    let thumb = egui::pos2(692., 50.);
    frame(&ctx, &result, &mut state, 0.3, pointer(thumb, true, false));
    frame(
        &ctx,
        &result,
        &mut state,
        0.32,
        vec![egui::Event::PointerMoved(egui::pos2(692., 430.))],
    );
    let last = frame(
        &ctx,
        &result,
        &mut state,
        0.34,
        pointer(egui::pos2(692., 430.), false, false),
    );
    assert!(last.text.iter().any(|(text, _, _)| text == "第 1999 行"));
    assert!(last.text.len() < 150);
}
#[test]
fn held_arrow_repeats_at_original_200ms_and_horizontal_scroll_keeps_row_selector_fixed() {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    let first = frame(&ctx, &result, &mut state, 0., vec![]);
    let initial_y = first
        .text
        .iter()
        .find(|(text, _, _)| text == "第 0 行")
        .unwrap()
        .1
        .y;
    let down = egui::pos2(692., 443.);
    frame(&ctx, &result, &mut state, 0.1, pointer(down, true, false));
    frame(&ctx, &result, &mut state, 0.29, vec![]);
    frame(&ctx, &result, &mut state, 0.31, vec![]);
    let held = frame(&ctx, &result, &mut state, 0.33, pointer(down, false, false));
    let y = held
        .text
        .iter()
        .find(|(text, _, _)| text == "第 0 行")
        .unwrap()
        .1
        .y;
    assert!((y - (initial_y - 20.)).abs() < 0.1);
    let ctx = egui::Context::default();
    let mut state = State::default();
    state.initialize(&result);
    state.widths[0] = 1000.;
    let wide = frame(&ctx, &result, &mut state, 0., vec![]);
    let initial_x = wide
        .text
        .iter()
        .find(|(text, _, _)| text == "missing")
        .unwrap()
        .1
        .x;
    let selector = egui::pos2(7., 42.);
    frame(
        &ctx,
        &result,
        &mut state,
        0.1,
        pointer(selector, true, false),
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.12,
        pointer(selector, false, false),
    );
    assert!((0..4).all(|column| state.contains(Cell { row: 0, column })));
    let right = egui::pos2(680., 442.);
    frame(&ctx, &result, &mut state, 0.2, pointer(right, true, false));
    let moved = frame(
        &ctx,
        &result,
        &mut state,
        0.22,
        pointer(right, false, false),
    );
    let x = moved
        .text
        .iter()
        .find(|(text, _, _)| text == "missing")
        .unwrap()
        .1
        .x;
    assert!((x - (initial_x - 10.)).abs() < 0.1);
    frame(
        &ctx,
        &result,
        &mut state,
        0.3,
        pointer(selector, true, true),
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.32,
        pointer(selector, false, true),
    );
    assert!(
        (0..4).all(|column| !state.contains(Cell { row: 0, column })),
        "same fixed selector must toggle the original selected row after horizontal scrolling"
    );
}
#[test]
fn mouse_resizes_columns_and_wheel_reaches_the_last_real_row() {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    let first = frame(&ctx, &result, &mut state, 0., vec![]);
    let cell_x = first
        .text
        .iter()
        .find(|(text, _, color)| text == "NULL" && *color == Color32::from_gray(153))
        .unwrap()
        .1
        .x;
    let header_y = first
        .text
        .iter()
        .find(|(text, _, _)| text == "missing")
        .unwrap()
        .1
        .y
        + 5.;
    let start = egui::pos2(cell_x - 7. + state.widths[0], header_y);
    let end = start + egui::vec2(40., 0.);
    frame(
        &ctx,
        &result,
        &mut state,
        0.02,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.04,
        vec![egui::Event::PointerMoved(end)],
    );
    frame(
        &ctx,
        &result,
        &mut state,
        0.06,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        (state.widths[0] - 112.).abs() < 0.1,
        "width={}",
        state.widths[0]
    );
    let mut last = frame(
        &ctx,
        &result,
        &mut state,
        0.1,
        vec![
            egui::Event::PointerMoved(egui::pos2(300., 200.)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0., -100000.),
                modifiers: Default::default(),
            },
        ],
    );
    for index in 0..30 {
        last = frame(
            &ctx,
            &result,
            &mut state,
            0.2 + f64::from(index) * 0.05,
            vec![],
        );
    }
    assert!(last.text.iter().any(|(text, _, _)| text == "第 1999 行"));
    assert!(last.text.len() < 150);
}
#[test]
fn real_result_grid_scrolls_past_500_keeps_null_style_and_copies_current_selection_without_os_clipboard()
 {
    let ctx = egui::Context::default();
    let result = fixture();
    let mut state = State::default();
    let first = frame(&ctx, &result, &mut state, 0., vec![]);
    assert!(first.text.len() < 150, "only visible rows are laid out");
    assert!(
        first
            .text
            .iter()
            .any(|(text, _, color)| text == "NULL" && *color == Color32::from_gray(153))
    );
    assert!(
        first
            .text
            .iter()
            .any(|(text, _, color)| text == "NULL" && *color == Color32::from_gray(51))
    );
    let point = first
        .text
        .iter()
        .find(|(text, _, _)| text == "第 0 行")
        .unwrap()
        .1
        + egui::vec2(3., 5.);
    for (time, pressed) in [(0.02, true), (0.04, false)] {
        frame(
            &ctx,
            &result,
            &mut state,
            time,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert_eq!(state.focus, Some(Cell { row: 0, column: 3 }));
    frame(
        &ctx,
        &result,
        &mut state,
        0.06,
        vec![key(egui::Key::End, true, false)],
    );
    assert_eq!(
        state.focus,
        Some(Cell {
            row: 1999,
            column: 3
        })
    );
    let mut last = first;
    for index in 0..30 {
        last = frame(
            &ctx,
            &result,
            &mut state,
            0.1 + f64::from(index) * 0.05,
            vec![],
        );
    }
    assert!(last.text.len() < 150);
    assert!(last.text.iter().any(|(text, _, _)| text == "第 1999 行"));
    let copied = frame(&ctx, &result, &mut state, 2., vec![egui::Event::Copy]);
    assert_eq!(copied.copied, vec!["第 1999 行"]);
    frame(
        &ctx,
        &result,
        &mut state,
        2.1,
        vec![key(egui::Key::Home, false, true)],
    );
    let copied = frame(&ctx, &result, &mut state, 2.2, vec![egui::Event::Copy]);
    assert_eq!(copied.copied, vec!["\t\tNULL\t第 1999 行"]);
}
