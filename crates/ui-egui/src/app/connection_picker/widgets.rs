use super::*;
pub(in crate::app) fn checkbox(
    ui: &mut Ui,
    rect: egui::Rect,
    id: egui::Id,
    checked: bool,
    partial: bool,
    label: Option<&str>,
    icons: &mut crate::icons::Icons,
) -> Option<bool> {
    let response = ui.interact(
        rect,
        id,
        if label.is_some() {
            egui::Sense::click()
        } else {
            egui::Sense::CLICK
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            label.unwrap_or(""),
        )
    });
    let bounds = if label.is_some() {
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 8.5, rect.center().y),
            egui::vec2(17., 17.),
        )
    } else {
        egui::Rect::from_center_size(rect.center(), egui::vec2(15., 15.))
    };
    ui.painter()
        .rect_filled(bounds, 2., Color32::from_gray(250));
    crate::native_dialog::gradient(
        ui,
        bounds.shrink(1.),
        Color32::from_gray(252),
        Color32::from_gray(if response.is_pointer_button_down_on() {
            200
        } else {
            222
        }),
    );
    ui.painter().rect_stroke(
        bounds,
        2.,
        egui::Stroke::new(1., Color32::from_gray(171)),
        egui::StrokeKind::Inside,
    );
    if partial {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(bounds.center(), egui::vec2(9., 2.)),
            0.,
            Color32::from_gray(51),
        );
    } else if checked {
        icons.paint(ui, "upstream/openjfx/modena-check.svg", bounds.shrink(2.));
    }
    if let Some(label) = label {
        ui.painter().text(
            egui::pos2(rect.left() + 24., rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.),
            Color32::BLACK,
        );
    }
    if !ui.is_enabled() {
        ui.painter()
            .rect_filled(bounds, 2., Color32::from_white_alpha(140));
        return None;
    }
    (response.clicked()
        || response.has_focus()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)))
    .then_some(!checked)
}

pub(in crate::app) fn button(ui: &mut Ui, rect: egui::Rect, label: &str, green: bool) -> bool {
    let response = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    ui.painter().rect_filled(
        rect,
        if green { 4 } else { 0 },
        if green {
            Color32::from_rgb(7, 193, 96)
        } else {
            Color32::from_gray(if response.hovered() { 247 } else { 248 })
        },
    );
    if !green {
        ui.painter().rect_stroke(
            rect,
            0,
            egui::Stroke::new(1., Color32::from_gray(204)),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.),
        if green {
            Color32::WHITE
        } else {
            Color32::BLACK
        },
    );
    response.clicked()
        || response.has_focus()
            && ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_checkbox_draws_its_real_embedded_mark_texture() {
        let ctx = egui::Context::default();
        let mut icons = crate::icons::Icons::default();
        for checked in [false, true] {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                checkbox(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(15., 15.), egui::vec2(16., 16.)),
                    egui::Id::new("checked-mark"),
                    checked,
                    false,
                    None,
                    &mut icons,
                );
            });
            let drawn=output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(mesh)if mesh.texture_id!=egui::TextureId::default()&&!mesh.vertices.is_empty()));
            assert_eq!(
                drawn, checked,
                "checked state must paint a mark, not silently miss its asset"
            );
            output.textures_delta.clear();
        }
    }
}
