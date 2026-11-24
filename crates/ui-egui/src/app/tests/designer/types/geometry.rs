//! Editing must preserve the real grid's geometry on every input frame.
use super::*;

fn grid_frame(app: &mut TomatoApp, id: &str, events: Vec<egui::Event>) -> Vec<egui::Rect> {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            events,
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    // Row selectors remain unedited and are painted in the same grid as editors.
    let selectors = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == egui::Color32::WHITE
                    && (rect.rect.width() - 15.).abs() < 0.01
                    && (rect.rect.height() - 30.).abs() < 0.01 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect();
    output.textures_delta.clear();
    selectors
}

fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}

#[test]
fn field_editors_and_type_dropdowns_leave_every_row_in_place() -> anyhow::Result<()> {
    for scale in [1., 1.25, 1.5, 2.] {
        let (mut app, id, _profile) = fixture("MYSQL")?;
        app.context.set_pixels_per_point(scale);
        let Panel::Sql { workbench, .. } = &mut app.tabs[0].panel else {
            unreachable!()
        };
        let state = workbench.designer.as_mut().unwrap();
        for index in 2..6 {
            state.append();
            let column = state.request.columns.last_mut().unwrap();
            column.name = format!("column_{index}");
            column.data_type = "INT".into();
        }
        state.select(Cell { row: 0, column: 0 }, false, false);
        grid_frame(&mut app, &id, vec![]);
        let baseline = grid_frame(&mut app, &id, vec![]);
        assert_eq!(baseline.len(), 6);
        for pair in baseline.windows(2) {
            assert!((pair[1].top() - pair[0].bottom()).abs() < 0.01);
        }
        let assert_frame = |app: &mut TomatoApp, events| {
            let actual = grid_frame(app, &id, events);
            assert_eq!(actual, baseline, "grid moved at display scale {scale}");
        };
        // F2 begins a real text editor. Check before/after every event, not only
        // the final state after a double click or after the popup closes.
        assert_frame(&mut app, vec![key(egui::Key::F2)]);
        assert!(draft(&app, &id).edit.is_some());
        for _ in 0..3 {
            assert_frame(&mut app, vec![]);
        }
        assert_frame(&mut app, vec![egui::Event::Text("new_name".into())]);
        assert_frame(&mut app, vec![key(egui::Key::Escape)]);
        assert_frame(&mut app, vec![]);
        let type_cell = egui::pos2(205., baseline[0].center().y);
        for pressed in [true, false] {
            assert_frame(&mut app, pointer(type_cell, pressed));
        }
        assert_frame(&mut app, vec![]);
        assert!(picker(&app, &id).open);
        // Toggle the arrow closed/open while the row remains in edit mode.
        let arrow = egui::pos2(275., baseline[0].center().y);
        for _ in 0..2 {
            for pressed in [true, false] {
                assert_frame(&mut app, pointer(arrow, pressed));
            }
            assert_frame(&mut app, vec![]);
        }
        assert_frame(&mut app, vec![key(egui::Key::ArrowDown)]);
        assert_frame(&mut app, vec![key(egui::Key::Enter)]);
        assert_frame(&mut app, vec![]);
        let second_name = egui::pos2(80., baseline[1].center().y);
        for pressed in [true, false] {
            assert_frame(&mut app, pointer(second_name, pressed));
        }
        assert_frame(&mut app, vec![key(egui::Key::F2)]);
        assert_frame(&mut app, vec![]);
        assert_frame(&mut app, vec![key(egui::Key::Enter)]);
        assert_frame(&mut app, vec![]);
    }
    Ok(())
}
