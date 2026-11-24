#[cfg(test)]
use super::*;
fn frame(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    icons: &mut crate::icons::Icons,
    events: Vec<egui::Event>,
) -> (Action, Vec<egui::epaint::ClippedShape>) {
    let mut action = Action::None;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(580., 500.),
            )),
            events,
            ..Default::default()
        },
        |ui| content(ui, dialog, icons, &mut action),
    );
    output.textures_delta.clear();
    (action, output.shapes)
}
fn click(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    icons: &mut crate::icons::Icons,
    x: f32,
    y: f32,
) -> Action {
    let pos = egui::pos2(x, y);
    frame(
        ctx,
        dialog,
        icons,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        ctx,
        dialog,
        icons,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    )
    .0
}
#[test]
fn original_row_spacing_button_bounds_and_independent_group_checks() -> anyhow::Result<()> {
    let plan = ImportPlan::parse(
        r#"[{"id":"g","name":"group"},{"id":"a","name":"first","type":"SSH","parentId":"g"},{"id":"b","name":"second","type":"MYSQL","parentId":"g"}]"#,
        &Store::default(),
    )?;
    let mut dialog = Dialog::new(plan);
    let ctx = egui::Context::default();
    let mut icons = crate::icons::Icons::default();
    let (_, shapes) = frame(&ctx, &mut dialog, &mut icons, vec![]);
    let mut ys = Vec::new();
    for shape in shapes {
        if let egui::Shape::Text(text) = shape.shape {
            let label = text.galley.text();
            if ["group", "first", "second"].contains(&label) {
                ys.push(text.pos.y);
            }
            if ["全选", "仅选择未重复项", "取消全选"].contains(&label) {
                assert_eq!(text.galley.rows.len(), 1);
                assert!(text.pos.y > 431. && text.pos.y < 454.);
            }
            if label == "选择要导入的项目：" {
                assert_eq!(text.pos, egui::pos2(15., 15.));
            }
        }
    }
    assert_eq!(ys.len(), 3);
    assert_eq!(ys[1] - ys[0], 30.);
    assert_eq!(ys[2] - ys[1], 30.);
    click(&ctx, &mut dialog, &mut icons, 50., 81.);
    assert_eq!(
        dialog.selected,
        BTreeSet::from([1, 2]),
        "group checks must not toggle children"
    );
    click(&ctx, &mut dialog, &mut icons, 35., 442.);
    assert_eq!(dialog.selected.len(), 3);
    click(&ctx, &mut dialog, &mut icons, 200., 442.);
    assert!(dialog.selected.is_empty());
    assert!(matches!(
        click(&ctx, &mut dialog, &mut icons, 520., 473.),
        Action::Apply
    ));
    Ok(())
}
