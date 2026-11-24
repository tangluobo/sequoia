//! Mouse selection uses the same toolbar and actor as normal query documents.
use super::*;
pub(super) fn choose_connection_through_toolbar(
    app: &mut TomatoApp,
    id: &str,
    name: &str,
) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    crate::theme::install(&ctx);
    let frame = |app: &mut TomatoApp, events: Vec<egui::Event>| {
        let index = app.tabs.iter().position(|tab| tab.id == id).unwrap();
        let mut intent = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                intent = crate::panels::render_with_connections(
                    ui,
                    &mut app.tabs[index],
                    &mut app.icons,
                    &app.store.connections,
                );
            },
        );
        let point = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == name
                {
                    Some(text.pos + egui::vec2(4., 6.))
                } else {
                    None
                }
            })
            .max_by(|a, b| a.y.total_cmp(&b.y));
        output.textures_delta.clear();
        if let Some(intent) = intent {
            app.handle_intent(id, intent);
        }
        point
    };
    let event = |point, pressed| {
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    frame(app, vec![]);
    let arrow = egui::pos2(137., 18.);
    for pressed in [true, false] {
        frame(app, event(arrow, pressed));
    }
    let mut point = None;
    for _ in 0..3 {
        point = frame(app, vec![]).or(point);
    }
    let point = point.ok_or_else(|| anyhow::anyhow!("original connection dropdown item"))?;
    for pressed in [true, false] {
        frame(app, event(point, pressed));
    }
    anyhow::ensure!(
        matches!(&app.tabs.iter().find(|tab|tab.id==id).unwrap().panel,Panel::Sql {connection,..}if connection.name==name),
        "actual toolbar did not select connection"
    );
    Ok(())
}
