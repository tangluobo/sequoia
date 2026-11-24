use super::*;

/// Reproduce the original 14px, 1.8px-stroke row toolbar without font glyphs.
pub(in crate::panels::sql) fn button(
    ui: &mut Ui,
    enabled: bool,
    icon: u8,
    label: &str,
) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2([25., 25., 27., 24., 27.][icon as usize], 23.0)),
        )
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(("table-toolbar", label)), response.rect));
    let origin = response.rect.center() - egui::vec2(7.0, 7.0);
    let stroke = egui::Stroke::new(
        1.8,
        if enabled {
            Color32::BLACK
        } else {
            Color32::from_gray(175)
        },
    );
    let line = |points: &[[f32; 2]]| {
        ui.painter().add(egui::Shape::line(
            points
                .iter()
                .map(|p| origin + egui::vec2(p[0], p[1]))
                .collect(),
            stroke,
        ));
    };
    match icon {
        0 => {
            line(&[[2.0, 7.0], [12.0, 7.0]]);
            line(&[[7.0, 2.0], [7.0, 12.0]]);
        }
        1 => line(&[[2.0, 7.0], [12.0, 7.0]]),
        2 => line(&[[2.0, 7.0], [6.0, 11.0], [13.0, 4.0]]),
        3 => {
            line(&[[3.0, 3.0], [11.0, 11.0]]);
            line(&[[11.0, 3.0], [3.0, 11.0]]);
        }
        _ => {
            let points: Vec<_> = (0..=20)
                .map(|step| {
                    let angle = 0.4 + step as f32 / 20.0 * 5.1;
                    [7.0 + angle.cos() * 5.0, 7.0 + angle.sin() * 5.0]
                })
                .collect();
            line(&points);
            line(&[[8.0, 1.0], [11.0, 3.0], [8.0, 5.0]]);
        }
    }
    response
}

/// Original Java SVG paths, rasterized once per display scale into tiny retained textures.
pub(super) fn action_icon(ui: &Ui, index: usize) -> egui::Image<'static> {
    use std::sync::Arc;
    type Icons = Arc<(f32, Vec<egui::TextureHandle>)>;
    let key = egui::Id::new("table-action-original-icons");
    let scale = ui.ctx().pixels_per_point();
    let cached = ui.ctx().data(|data| data.get_temp::<Icons>(key));
    let cache = if let Some(cache) = cached.filter(|cache| cache.0 == scale) {
        cache
    } else {
        let mut textures = Vec::new();
        for (index, (path, color)) in [
            ("M2 3c0-1 3-2 6-2s6 1 6 2-3 2-6 2-6-1-6-2zm0 3c1 1 3 2 6 2s5-1 6-2v3c0 1-3 2-6 2s-6-1-6-2V6zm0 6c1 1 3 2 6 2s5-1 6-2v2c0 1-3 2-6 2s-6-1-6-2v-3z", "#5D8B63"),
            ("M2 1h10l3 3v12H2z M4 6h9v1H4z M4 9h9v1H4z M4 12h7v1H4z", "#4D78A8"),
            ("M1 2h14L9.5 8v5l-3 2V8z", "#3E9FE6"),
            ("M2 1h2v10h3l-4 4-3-4h2z M9 2h7v1.5H9z M9 6h6v1.5H9z M9 10h5v1.5H9z", "#4D8EC8"),
            ("M1 2h11v4h3v9H1z M8 0v8l3-3v2L7 11 3 7V5l3 3V0z", "#2E9A72"),
            ("M1 2h11v4h3v9H1z M6 11V3L3 6V4l4-4 4 4v2L8 3v8z", "#E3912D"),
        ].into_iter().enumerate() {
            let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 16 16'><path d='{path}' fill='{color}' transform='translate(1.2 1.2) scale(0.85)'/></svg>");
            let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default()).expect("static original SVG");
            let pixels = (16. * scale).ceil() as u32;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels).expect("16px icon");
            resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
            textures.push(ui.ctx().load_texture(format!("table-action-{index}"),
                egui::ColorImage::from_rgba_premultiplied([pixels as usize,pixels as usize],pixmap.data()), egui::TextureOptions::LINEAR));
        }
        let cache = Arc::new((scale, textures));
        ui.ctx()
            .data_mut(|data| data.insert_temp(key, cache.clone()));
        cache
    };
    egui::Image::new((cache.1[index].id(), egui::vec2(16., 16.)))
}

pub(super) fn menu(ui: &mut Ui, index: usize, label: &str, contents: impl FnOnce(&mut Ui)) {
    let width = [108., 85., 54., 56.][index];
    let response = ui.add_sized([width, 31.], egui::Button::new("").frame(false));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let icon_x = [12., 12., 5., 6.][index];
    action_icon(ui, index).paint_at(
        ui,
        egui::Rect::from_min_size(
            response.rect.min + egui::vec2(icon_x, 7.5),
            egui::vec2(16., 16.),
        ),
    );
    ui.painter().text(
        response.rect.left_center() + egui::vec2([30., 30., 24., 26.][index], 0.),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.),
        Color32::from_gray(34),
    );
    if index < 2 {
        let center = egui::pos2(response.rect.right() - 19., response.rect.center().y);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(-3., -1.5),
                center + egui::vec2(3., -1.5),
                center + egui::vec2(0., 1.5),
            ],
            Color32::from_gray(85),
            egui::Stroke::NONE,
        ));
    }
    egui::Popup::menu(&response).show(contents);
}
pub(super) fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8., 27.), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1., Color32::from_gray(215)),
    );
}
