use super::*;
pub(super) fn render(ui: &mut Ui, state: &mut State, id: &str) -> Option<Action> {
    state.diagram();
    let diagram = state.diagram.as_ref()?;
    let mut clicked = None;
    let mut action = None;
    let bounds = ui.available_rect_before_wrap();
    ui.painter().rect_filled(bounds, 0, Color32::from_gray(250));
    let output = egui::ScrollArea::both()
        .id_salt(("object-er", id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (canvas, _) = ui.allocate_exact_size(
                vec2(
                    2000_f32.max(diagram.columns as f32 * 280.),
                    1500_f32.max(diagram.tables.len().div_ceil(diagram.columns) as f32 * 180.),
                ),
                Sense::hover(),
            );
            let green = Color32::from_rgb(7, 193, 96);
            for &(source, target, index) in &diagram.links {
                let start = diagram.rect(canvas.min, source).center();
                let end = diagram.rect(canvas.min, target).center();
                let bounds = Rect::from_two_pos(start, end).expand(4.);
                if !bounds.intersects(ui.clip_rect()) {
                    continue;
                }
                let y = (start.y + end.y) / 2.;
                let points = [start, pos2(start.x, y), pos2(end.x, y), end];
                let pointer = ui.input(|input| input.pointer.hover_pos());
                let hovered = pointer
                    .is_some_and(|point| bounds.contains(point) && near_curve(point, points));
                if hovered {
                    let key = &state.catalog.foreign_keys[index];
                    ui.interact(
                        Rect::from_center_size(pointer.unwrap(), vec2(8., 8.)),
                        Id::new(("object-fk", id, index)),
                        Sense::hover(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_ui(|ui| {
                        ui.label(format!(
                            "{}.{}  →  {}.{}",
                            key.table.name,
                            key.columns.join(","),
                            key.referenced.name,
                            key.referenced_columns.join(",")
                        ));
                    });
                }
                ui.painter()
                    .add(egui::epaint::CubicBezierShape::from_points_stroke(
                        points,
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(if hovered { 2.5 } else { 1.5 }, green),
                    ));
            }
            for (ordinal, &index) in diagram.tables.iter().enumerate() {
                let rect = diagram.rect(canvas.min, ordinal);
                if !rect.intersects(ui.clip_rect()) {
                    continue;
                }
                ui.painter().rect_filled(
                    rect.translate(vec2(1., 1.)).expand(1.),
                    4,
                    Color32::from_black_alpha(35),
                );
                ui.painter().rect(
                    rect,
                    4,
                    green,
                    Stroke::new(1., Color32::from_gray(136)),
                    StrokeKind::Inside,
                );
                ui.painter()
                    .with_clip_rect(rect.intersect(ui.clip_rect()))
                    .text(
                        rect.left_center() + vec2(9., 0.),
                        Align2::LEFT_CENTER,
                        &state.catalog.objects[index].table.name,
                        bold(ui),
                        Color32::WHITE,
                    );
                let response = ui
                    .interact(rect, Id::new(("object-er-box", id, index)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    clicked = Some(index);
                }
                if response.double_clicked() {
                    action = Some(Action::Open(index));
                }
            }
        });
    scrollbars(ui, state, output, bounds);
    if let Some(index) = clicked {
        state.click(index, false, false);
        ui.memory_mut(|memory| memory.request_focus(Id::new(("objects-focus", id))));
    }
    action
}
fn near_curve(point: Pos2, curve: [Pos2; 4]) -> bool {
    let mut previous = curve[0];
    for step in 1..=24 {
        let t = step as f32 / 24.;
        let u = 1. - t;
        let position = pos2(
            u * u * u * curve[0].x
                + 3. * u * u * t * curve[1].x
                + 3. * u * t * t * curve[2].x
                + t * t * t * curve[3].x,
            u * u * u * curve[0].y
                + 3. * u * u * t * curve[1].y
                + 3. * u * t * t * curve[2].y
                + t * t * t * curve[3].y,
        );
        let edge = position - previous;
        let length = edge.length_sq();
        let nearest = if length > 0. {
            previous + edge * ((point - previous).dot(edge) / length).clamp(0., 1.)
        } else {
            previous
        };
        if point.distance_sq(nearest) <= 16. {
            return true;
        }
        previous = position;
    }
    false
}
