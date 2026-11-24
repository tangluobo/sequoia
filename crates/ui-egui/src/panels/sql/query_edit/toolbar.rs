//! Original SVG strokes, square caps and measured bounds from the JavaFX footer.
use super::*;
use std::sync::Arc;
struct Asset {
    texture: egui::TextureHandle,
    size: egui::Vec2,
    offset: egui::Vec2,
    width: f32,
}
struct Cache {
    scale: f32,
    assets: Vec<Asset>,
}
fn assets(ctx: &egui::Context) -> Option<Arc<Cache>> {
    let key = egui::Id::new("sql-query-result-footer-strokes");
    let scale = ctx.pixels_per_point();
    if let Some(cache) = ctx.data(|data| data.get_temp::<Arc<Cache>>(key))
        && cache.scale == scale
    {
        return Some(cache);
    }
    let mut assets = Vec::with_capacity(3);
    for (index, path, view, size, offset, width) in [
        (
            0,
            "M2 7H12",
            "0.6 5.6 12.8 2.8",
            egui::vec2(12.8, 2.8),
            egui::vec2(6.0, 10.0),
            25.0,
        ),
        (
            1,
            "M2 7L6 11L13 4",
            "0.2272078 2.2272078 14.5455844 10.5455844",
            egui::vec2(14.545584, 10.545584),
            egui::vec2(6.0, 6.0),
            27.0,
        ),
        (
            2,
            "M3 3L11 11 M11 3L3 11",
            "1.2272078 1.2272078 11.5455844 11.5455844",
            egui::vec2(11.545584, 11.545584),
            egui::vec2(6.0, 5.0),
            24.0,
        ),
    ] {
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' viewBox='{view}'><path d='{path}' fill='none' stroke='black' stroke-width='1.8' stroke-linecap='square' stroke-linejoin='miter'/></svg>",
            size.x, size.y
        );
        let tree =
            resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default()).ok()?;
        let pixel_size = egui::vec2((size.x * scale).ceil(), (size.y * scale).ceil());
        let mut pixels = resvg::tiny_skia::Pixmap::new(pixel_size.x as u32, pixel_size.y as u32)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixels.as_mut(),
        );
        let image = egui::ColorImage::from_rgba_premultiplied(
            [pixel_size.x as usize, pixel_size.y as usize],
            pixels.data(),
        );
        let texture = ctx.load_texture(
            format!("original-query-footer-{index}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        assets.push(Asset {
            texture,
            size: pixel_size / scale,
            offset,
            width,
        });
    }
    let cache = Arc::new(Cache { scale, assets });
    ctx.data_mut(|data| data.insert_temp(key, cache.clone()));
    Some(cache)
}
pub(super) fn button(ui: &mut Ui, enabled: bool, index: usize, label: &str) -> egui::Response {
    let cache = assets(ui.ctx());
    let asset = cache.as_ref().and_then(|cache| cache.assets.get(index));
    let width = asset.map_or([25.0, 27.0, 24.0][index], |asset| asset.width);
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2(width, 22.0)),
        )
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(("query-toolbar", label)), response.rect));
    if let Some(asset) = asset {
        let rect = egui::Rect::from_min_size(response.rect.min + asset.offset, asset.size);
        ui.painter().image(
            asset.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            if enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(102)
            },
        );
    }
    response
}
pub(super) fn controls(ui: &mut Ui, state: &mut State, busy: bool, rect: egui::Rect) -> bool {
    ui.painter().rect_filled(rect, 0, Color32::from_gray(243));
    for y in [rect.top() + 0.5, rect.bottom() - 0.5] {
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0, Color32::from_gray(215)),
        );
    }
    let controls = egui::Rect::from_min_size(
        rect.min + egui::vec2(5.0, 3.0),
        egui::vec2((rect.width() - 10.0).max(0.0), 22.0),
    );
    let mut save = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(controls)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
            let editable = !busy
                && !state.saving
                && state
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.target.plan.is_editable());
            let delete = editable
                && state.focus.is_some()
                && state
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.target.plan.can_delete());
            if button(ui, delete, 0, "删除选中记录").clicked()
                && let Some(editing) = &mut state.editing
            {
                editing.confirm_delete = true;
            }
            let dirty = state
                .editing
                .as_ref()
                .is_some_and(|editing| editing.dirty());
            save = button(ui, editable && dirty, 1, "保存修改").clicked();
            if button(ui, !busy && dirty, 2, "取消修改").clicked()
                && let Some(editing) = &mut state.editing
            {
                editing.release(ui.ctx());
                editing.cancel();
            }
        },
    );
    save
}
