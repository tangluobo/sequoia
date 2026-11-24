//! Native event loops can deliver a press, several moves and release in one frame.
use super::*;

fn button(point: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
fn rectangle(app: &TomatoApp, id: &str, rows: std::ops::RangeInclusive<usize>, columns: usize) {
    let state = draft(app, id);
    for row in 0..state.request.columns.len() {
        for column in 0..8 {
            assert_eq!(
                state.selected(row, column),
                rows.contains(&row) && column < columns,
                "row {row}, column {column}"
            );
        }
    }
    assert!(state.edit.is_none());
    assert!(!state.dirty);
}

#[test]
fn coalesced_press_moves_and_release_keep_the_original_anchor() -> anyhow::Result<()> {
    for reverse in [false, true] {
        let (mut app, id, _profile) = setup(6)?;
        let start = egui::pos2(
            if reverse { 342. } else { 80. },
            if reverse { 193. } else { 103. },
        );
        let end = egui::pos2(
            if reverse { 80. } else { 342. },
            if reverse { 103. } else { 193. },
        );
        run(
            &mut app,
            &id,
            vec![
                egui::Event::PointerMoved(start),
                button(start, true),
                egui::Event::PointerMoved(egui::pos2(210., 133.)),
                egui::Event::PointerMoved(end),
                button(end, false),
            ],
            egui::Modifiers::NONE,
        );
        rectangle(&app, &id, 0..=3, 3);
        assert!(draft(&app, &id).drag.is_none());
    }
    Ok(())
}

#[test]
fn selecting_then_holding_captures_the_grid_and_extends_without_editing() -> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    click_mod(&mut app, &id, 80., 1, egui::Modifiers::NONE);
    let start = egui::pos2(80., 133.);
    pointer(&mut app, &id, start, Some(true), egui::Modifiers::NONE);
    // Wait longer than egui's click timeout before moving to another cell.
    for _ in 0..90 {
        run(&mut app, &id, vec![], egui::Modifiers::NONE);
    }
    assert!(
        app.context.dragged_id().is_some(),
        "The cell must capture the held pointer"
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(490., 223.),
        None,
        egui::Modifiers::NONE,
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(490., 223.),
        Some(false),
        egui::Modifiers::NONE,
    );
    rectangle(&app, &id, 1..=4, 6);
    assert!(draft(&app, &id).drag.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    Ok(())
}

#[test]
fn coalesced_row_selector_drag_and_press_outside_keep_their_own_origins() -> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    let start = egui::pos2(7., 133.);
    let end = egui::pos2(400., 223.);
    run(
        &mut app,
        &id,
        vec![
            button(start, true),
            egui::Event::PointerMoved(end),
            button(end, false),
        ],
        egui::Modifiers::NONE,
    );
    rectangle(&app, &id, 1..=4, 8);
    let selection = draft(&app, &id).selected_rows();
    run(
        &mut app,
        &id,
        vec![
            button(egui::pos2(80., 40.), true),
            egui::Event::PointerMoved(start),
            button(start, false),
        ],
        egui::Modifiers::NONE,
    );
    assert_eq!(draft(&app, &id).selected_rows(), selection);
    assert!(draft(&app, &id).drag.is_none());
    Ok(())
}

#[test]
fn main_shell_selected_cell_holds_and_drag_events_keep_content_coordinates() -> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    let ctx = app.context.clone();
    let render = |app: &mut TomatoApp, events: Vec<egui::Event>| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        output
    };
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let start = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == "id"
            {
                Some(text.pos + egui::vec2(20., 6.))
            } else {
                None
            }
        })
        .expect("Field name must be visible inside the real main shell");
    for pressed in [true, false] {
        render(&mut app, vec![button(start, pressed)]);
    }
    render(&mut app, vec![button(start, true)]);
    for _ in 0..90 {
        render(&mut app, vec![]);
    }
    assert!(ctx.dragged_id().is_some());
    let end = start + egui::vec2(260., 90.);
    render(
        &mut app,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );
    rectangle(&app, &id, 0..=3, 3);
    // A new fast reverse gesture must begin at its actual pressed cell, even
    // when the last pointer event is already over a different widget.
    render(
        &mut app,
        vec![
            button(end, true),
            egui::Event::PointerMoved(start),
            button(start, false),
        ],
    );
    rectangle(&app, &id, 0..=3, 3);
    assert!(app.pending.is_empty());
    assert!(!egui::Popup::is_any_open(&ctx));
    Ok(())
}
