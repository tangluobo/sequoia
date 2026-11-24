use super::*;

#[test]
fn moving_to_an_already_visible_type_keeps_the_dropdown_scroll_and_draft_unchanged()
-> anyhow::Result<()> {
    let (mut app, id, _directory) = fixture("MYSQL")?;
    click(&mut app, &id, 205., 133.);
    let position = |app: &mut TomatoApp| -> anyhow::Result<egui::Pos2> {
        let rect = popup(app, &id);
        let ctx = app.context.clone();
        let output = ctx.run_ui(
            egui::RawInput {
                time: Some(ctx.input(|input| input.time) + 0.05),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(896., 650.),
                )),
                ..Default::default()
            },
            |_| draw(app, &ctx, &id),
        );
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == "VARCHAR"
                    && rect.contains(text.pos)
                {
                    Some(text.pos)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("current type must remain visible in its dropdown"))
    };
    let before = position(&mut app)?;
    let revision = draft(&app, &id).revision;
    frame(&mut app, &id, vec![key(egui::Key::ArrowUp)]);
    assert_ne!(
        draft(&app, &id).types()[picker(&app, &id).focused.unwrap()],
        "VARCHAR"
    );
    let after = position(&mut app)?;
    assert!(
        (before.y - after.y).abs() < 0.1,
        "visible candidates must not jump on ArrowUp: {before:?} -> {after:?}"
    );
    frame(&mut app, &id, vec![key(egui::Key::ArrowDown)]);
    assert!((before.y - position(&mut app)?.y).abs() < 0.1);
    assert_eq!(draft(&app, &id).request.columns[1].data_type, "VARCHAR");
    assert_eq!(draft(&app, &id).revision, revision);
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    Ok(())
}
