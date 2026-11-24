//! Real press/move/release frames, including the platform's modifier state.
use super::*;
mod gestures;

fn setup(count: usize) -> anyhow::Result<(TomatoApp, String, tempfile::TempDir)> {
    let (mut app, id, profile) = fixture("MYSQL")?;
    let Panel::Sql { workbench, .. } = &mut app.tabs[0].panel else {
        unreachable!()
    };
    let state = workbench.designer.as_mut().unwrap();
    for row in 2..count {
        state.append();
        let column = state.request.columns.last_mut().unwrap();
        column.name = format!("column_{row}");
        column.data_type = "INT".into();
    }
    state.clear_selection();
    state.dirty = false;
    run(&mut app, &id, vec![], egui::Modifiers::NONE);
    Ok((app, id, profile))
}
fn run(
    app: &mut TomatoApp,
    id: &str,
    mut events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let ctx = app.context.clone();
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
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
    output.textures_delta.clear();
    output
}
fn pointer(
    app: &mut TomatoApp,
    id: &str,
    point: egui::Pos2,
    pressed: Option<bool>,
    modifiers: egui::Modifiers,
) {
    let mut events = vec![egui::Event::PointerMoved(point)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    run(app, id, events, modifiers);
}
fn click_mod(app: &mut TomatoApp, id: &str, x: f32, row: usize, modifiers: egui::Modifiers) {
    let point = egui::pos2(x, 103. + row as f32 * 30.);
    for pressed in [true, false] {
        pointer(app, id, point, Some(pressed), modifiers);
    }
    run(app, id, vec![], modifiers);
}
fn assert_rows(app: &TomatoApp, id: &str, rows: &[usize], whole: bool) {
    let state = draft(app, id);
    assert_eq!(state.selected_rows(), rows);
    if whole {
        for row in rows {
            for column in 0..8 {
                assert!(state.selected(*row, column), "row {row}, column {column}");
            }
        }
    }
    assert!(
        state.edit.is_none(),
        "Selection must not enter a cell editor"
    );
}
#[test]
fn selector_ctrl_shift_and_drag_keep_selection_after_release_and_paint_each_arrow()
-> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    let revision = draft(&app, &id).revision;
    click_mod(&mut app, &id, 7., 0, egui::Modifiers::NONE);
    click_mod(&mut app, &id, 7., 2, egui::Modifiers::CTRL);
    assert_rows(&app, &id, &[0, 2], true);
    click_mod(&mut app, &id, 7., 0, egui::Modifiers::CTRL);
    assert_rows(&app, &id, &[2], true);
    click_mod(&mut app, &id, 7., 4, egui::Modifiers::SHIFT);
    assert_rows(&app, &id, &[2, 3, 4], true);
    // A row-header gesture keeps selecting whole rows after crossing into cells.
    pointer(
        &mut app,
        &id,
        egui::pos2(7., 223.),
        Some(true),
        egui::Modifiers::NONE,
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(400., 133.),
        None,
        egui::Modifiers::NONE,
    );
    assert_rows(&app, &id, &[1, 2, 3, 4], true);
    pointer(
        &mut app,
        &id,
        egui::pos2(400., 193.),
        None,
        egui::Modifiers::NONE,
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(400., 193.),
        Some(false),
        egui::Modifiers::NONE,
    );
    assert_rows(&app, &id, &[3, 4], true);
    assert!(draft(&app, &id).drag.is_none());
    let output = run(&mut app, &id, vec![], egui::Modifiers::NONE);
    let arrows=output.shapes.iter().filter(|shape| matches!(&shape.shape,egui::Shape::Path(path) if path.fill==egui::Color32::BLACK && path.points.len()==3 && path.points.iter().all(|point| point.x<15. && point.y>90.))).count();
    assert_eq!(
        arrows, 2,
        "Every selected row must show the Java selector arrow"
    );
    let blue=output.shapes.iter().filter(|shape| matches!(&shape.shape,egui::Shape::Rect(rect) if rect.fill==egui::Color32::from_rgb(53,146,203) && rect.rect.height()==30.)).count();
    assert_eq!(
        blue, 16,
        "Both rows must be visibly highlighted across all eight cells"
    );
    assert!(!draft(&app, &id).dirty);
    assert_eq!(draft(&app, &id).revision, revision);
    Ok(())
}
#[test]
fn cell_ctrl_toggle_and_rectangular_drag_do_not_edit_types_or_checkboxes() -> anyhow::Result<()> {
    for scale in [1., 1.25, 1.5, 2.] {
        let (mut app, id, _profile) = setup(6)?;
        app.context.set_pixels_per_point(scale);
        let columns = draft(&app, &id).request.columns.clone();
        click_mod(&mut app, &id, 80., 0, egui::Modifiers::NONE);
        click_mod(&mut app, &id, 80., 2, egui::Modifiers::CTRL);
        assert_rows(&app, &id, &[0, 2], false);
        click_mod(&mut app, &id, 80., 0, egui::Modifiers::CTRL);
        assert_rows(&app, &id, &[2], false);
        pointer(
            &mut app,
            &id,
            egui::pos2(80., 103.),
            Some(true),
            egui::Modifiers::NONE,
        );
        pointer(
            &mut app,
            &id,
            egui::pos2(342., 163.),
            None,
            egui::Modifiers::NONE,
        );
        for row in 0..6 {
            for column in 0..8 {
                assert_eq!(
                    draft(&app, &id).selected(row, column),
                    row <= 2 && column <= 2,
                    "Scale {scale}, row {row}, column {column}"
                );
            }
        }
        // Shrinking back must remove cells that are no longer in the rectangle.
        pointer(
            &mut app,
            &id,
            egui::pos2(80., 133.),
            None,
            egui::Modifiers::NONE,
        );
        pointer(
            &mut app,
            &id,
            egui::pos2(80., 133.),
            Some(false),
            egui::Modifiers::NONE,
        );
        assert_rows(&app, &id, &[0, 1], false);
        for row in 0..2 {
            for column in 0..8 {
                assert_eq!(draft(&app, &id).selected(row, column), column == 0);
            }
        }
        click_mod(&mut app, &id, 342., 3, egui::Modifiers::SHIFT);
        for row in 0..4 {
            for column in 0..8 {
                assert_eq!(draft(&app, &id).selected(row, column), column <= 2);
            }
        }
        assert!(draft(&app, &id).request.columns == columns);
        assert!(!draft(&app, &id).dirty);
        assert!(!egui::Popup::is_any_open(&app.context));
        pointer(
            &mut app,
            &id,
            egui::pos2(80., 103.),
            Some(true),
            egui::Modifiers::NONE,
        );
        pointer(
            &mut app,
            &id,
            egui::pos2(490., 193.),
            None,
            egui::Modifiers::NONE,
        );
        pointer(
            &mut app,
            &id,
            egui::pos2(490., 193.),
            Some(false),
            egui::Modifiers::NONE,
        );
        assert!(
            draft(&app, &id).request.columns == columns,
            "Dragging over checkbox/primary-key cells must not toggle them"
        );
        assert!(!draft(&app, &id).dirty);
    }
    Ok(())
}
#[test]
fn dragging_inside_an_active_text_editor_keeps_text_selection_owned_by_the_editor()
-> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    click_mod(&mut app, &id, 80., 0, egui::Modifiers::NONE);
    run(
        &mut app,
        &id,
        vec![key(egui::Key::F2)],
        egui::Modifiers::NONE,
    );
    run(&mut app, &id, vec![], egui::Modifiers::NONE);
    assert!(draft(&app, &id).edit.is_some());
    pointer(
        &mut app,
        &id,
        egui::pos2(30., 103.),
        Some(true),
        egui::Modifiers::NONE,
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(140., 133.),
        None,
        egui::Modifiers::NONE,
    );
    pointer(
        &mut app,
        &id,
        egui::pos2(140., 133.),
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(draft(&app, &id).edit.is_some());
    assert!(draft(&app, &id).drag.is_none());
    assert_eq!(draft(&app, &id).selected_rows(), [0]);
    assert!(!draft(&app, &id).dirty);
    Ok(())
}
#[test]
fn selector_context_menu_preserves_batch_copy_and_delete_and_blank_clears() -> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(6)?;
    click_mod(&mut app, &id, 7., 1, egui::Modifiers::NONE);
    click_mod(&mut app, &id, 7., 4, egui::Modifiers::CTRL);
    let point = egui::pos2(7., 133.);
    for pressed in [true, false] {
        run(
            &mut app,
            &id,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            egui::Modifiers::NONE,
        );
    }
    assert_rows(&app, &id, &[1, 4], true);
    assert!(egui::Popup::is_any_open(&app.context));
    run(
        &mut app,
        &id,
        vec![key(egui::Key::Escape)],
        egui::Modifiers::NONE,
    );
    run(&mut app, &id, vec![], egui::Modifiers::NONE);
    run(
        &mut app,
        &id,
        vec![egui::Event::Copy],
        egui::Modifiers::NONE,
    );
    let copied = draft(&app, &id)
        .copied_fields
        .as_ref()
        .expect("Copy uses selected fields");
    let copied: serde_json::Value = serde_json::from_str(copied)?;
    assert_eq!(copied.as_array().unwrap().len(), 2);
    assert_eq!(copied[0]["字段名"], "label");
    assert_eq!(copied[1]["字段名"], "column_4");
    // This is the real toolbar action; it changes only the unsaved draft.
    click(&mut app, &id, 280., 15.);
    let state = draft(&app, &id);
    assert_eq!(
        state
            .request
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "column_2", "column_3", "column_5"]
    );
    assert_eq!(state.origins.len(), 4);
    assert!(state.dirty);
    assert!(state.status.contains("2 个字段"));
    assert!(
        app.pending.is_empty(),
        "Selection and deleting a draft must not save to the database"
    );
    click_mod(&mut app, &id, 7., 1, egui::Modifiers::NONE);
    for pressed in [true, false] {
        pointer(
            &mut app,
            &id,
            egui::pos2(880., 300.),
            Some(pressed),
            egui::Modifiers::NONE,
        );
    }
    assert_rows(&app, &id, &[], true);
    Ok(())
}
#[test]
fn drag_selection_scrolls_past_virtual_rows_and_stops_on_release_or_pointer_loss()
-> anyhow::Result<()> {
    let (mut app, id, _profile) = setup(1000)?;
    pointer(
        &mut app,
        &id,
        egui::pos2(7., 103.),
        Some(true),
        egui::Modifiers::NONE,
    );
    // Grid bottom is derived from painted selector cells, not a guessed viewport size.
    let output = run(&mut app, &id, vec![], egui::Modifiers::NONE);
    let bottom = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Rect(rect) = &shape.shape {
                (rect.rect.width() == 15. && rect.rect.height() == 30. && rect.rect.top() > 85.)
                    .then_some(rect.rect.bottom().min(shape.clip_rect.bottom()))
            } else {
                None
            }
        })
        .fold(0., f32::max);
    let point = egui::pos2(7., bottom - 4.);
    for _ in 0..90 {
        pointer(&mut app, &id, point, None, egui::Modifiers::NONE);
    }
    let rows = draft(&app, &id).selected_rows();
    assert_eq!(rows.first(), Some(&0));
    assert!(
        rows.last().copied().unwrap() > 15,
        "Selection must extend beyond the initial viewport: {rows:?}"
    );
    assert!(!draft(&app, &id).dirty);
    pointer(&mut app, &id, point, Some(false), egui::Modifiers::NONE);
    let rows = draft(&app, &id).selected_rows();
    for _ in 0..10 {
        run(&mut app, &id, vec![], egui::Modifiers::NONE);
    }
    assert_eq!(draft(&app, &id).selected_rows(), rows);
    assert!(draft(&app, &id).drag.is_none());
    pointer(
        &mut app,
        &id,
        egui::pos2(80., 103.),
        Some(true),
        egui::Modifiers::NONE,
    );
    assert!(draft(&app, &id).drag.is_some());
    // PointerGone cancels a captured gesture instead of leaving it armed.
    run(
        &mut app,
        &id,
        vec![egui::Event::PointerGone],
        egui::Modifiers::NONE,
    );
    assert!(draft(&app, &id).drag.is_none());
    Ok(())
}
