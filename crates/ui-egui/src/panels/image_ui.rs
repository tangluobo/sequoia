use crate::tabs::ssh::{
    Workspace,
    images::{Preview, PreviewAction},
};
use egui::Color32;
#[cfg(test)]
mod tests;

pub(super) fn render(ctx: &egui::Context, workspace: &mut Workspace) {
    for preview in &mut workspace.previews {
        if preview.closed {
            continue;
        }
        let id = egui::ViewportId::from_hash_of(("file-image", &preview.id));
        let capture = ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-image-fixture")))
            .unwrap_or_default();
        let chrome = egui::vec2(800., 600.) - preview.inner_size.unwrap_or(egui::vec2(800., 600.));
        let mut builder = egui::ViewportBuilder::default()
            .with_title(&preview.title)
            .with_inner_size(preview.inner_size.unwrap_or(egui::vec2(800., 600.)))
            .with_min_inner_size(egui::vec2(600., 500.) - chrome);
        if !preview.positioned {
            if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
                builder = builder.with_position(rect.center() - egui::vec2(400., 300.));
            }
            preview.positioned = true;
        }
        if capture {
            builder = builder.with_active(false);
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
        }
        ctx.show_viewport_immediate(id, builder, |ui, _| {
            if capture&&let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==id{Some(image.clone())}else{None})) {
                ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));
                if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from){
                    let state=serde_json::json!({"ready":preview.texture.is_some(),"title":preview.title,"width":preview.dimensions.x,"height":preview.dimensions.y,"scale":preview.scale,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"toolbarHeight":33,"error":preview.error});
                    let _=std::fs::write(path.with_extension("state.json"),state.to_string());
                }
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
            if preview.inner_size.is_none()
                && let Some((inner, outer)) =
                    ui.input(|input| input.viewport().inner_rect.zip(input.viewport().outer_rect))
            {
                let chrome = (outer.size() - inner.size()).max(egui::Vec2::ZERO);
                let size = egui::vec2(800., 600.) - chrome;
                preview.inner_size = Some(size);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
                        egui::vec2(600., 500.) - chrome,
                    ));
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
            ui.painter()
                .rect_filled(ui.max_rect(), 0., Color32::from_gray(43));
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(11.));
            if ui.input(|input| input.viewport().close_requested()) {
                preview.closed = true;
                preview.request.cancel.cancel();
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            if let Some(animation) = &preview.animation {
                if let Some(frame) = animation.frame()
                    && let Some(texture) = &mut preview.texture
                {
                    texture.set(
                        egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width as usize, frame.height as usize],
                            &frame.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    );
                }
                let error = animation.error();
                let finished = animation.is_finished();
                if let Some(error) = error {
                    preview.error = error;
                    preview.texture = None;
                    preview.animation = None;
                } else if finished {
                    // A worker can publish its last frame between frame() and
                    // is_finished(); retain that final image before releasing it.
                    if let Some(frame)=animation.frame()&&let Some(texture)=&mut preview.texture {
                        texture.set(egui::ColorImage::from_rgba_unmultiplied([frame.width as usize,frame.height as usize],&frame.rgba),egui::TextureOptions::LINEAR);
                    }
                    preview.animation = None;
                }
            }
            if !preview.error.is_empty() {
                center_text(ui, &preview.error);
                return;
            }
            if preview.texture.is_none() {
                let rect =
                    egui::Rect::from_center_size(ui.max_rect().center(), egui::vec2(60., 60.));
                ui.place(rect, egui::Spinner::new().size(60.));
                return;
            }
            ui.add_enabled_ui(!preview.confirm_delete && preview.notice.is_none(), |ui| {
                ready(ui, preview)
            });
            if capture&&let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from){
                let state=serde_json::json!({"ready":preview.texture.is_some(),"title":preview.title,"width":preview.dimensions.x,"height":preview.dimensions.y,"scale":preview.scale,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"toolbarHeight":33,"error":preview.error});
                let _=std::fs::write(path.with_extension("state.json"),state.to_string());
            }
        });
        if preview.closed {
            continue;
        }
        if preview.confirm_delete {
            confirm(ctx, preview);
        }
        if preview.notice.is_some() {
            notice(ctx, preview);
        }
    }
}
fn center_text(ui: &egui::Ui, text: &str) {
    ui.painter().text(
        ui.max_rect().center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.),
        Color32::from_rgb(255, 107, 107),
    );
}
fn toolbar_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: &str,
    enabled: bool,
    danger: bool,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        ui.spacing_mut().interact_size = egui::vec2(1., 1.);
        let visuals = &mut ui.visuals_mut().widgets;
        for widget in [
            &mut visuals.inactive,
            &mut visuals.hovered,
            &mut visuals.active,
        ] {
            widget.weak_bg_fill = Color32::from_gray(234);
            widget.bg_fill = Color32::from_gray(234);
            widget.fg_stroke.color = if danger {
                Color32::from_rgb(255, 107, 107)
            } else {
                Color32::BLACK
            };
            widget.corner_radius = egui::CornerRadius::same(2);
        }
        ui.put(
            rect,
            egui::Button::new(egui::RichText::new(text).color(if danger {
                Color32::from_rgb(255, 107, 107)
            } else {
                Color32::BLACK
            }))
            .min_size(rect.size())
            .wrap_mode(egui::TextWrapMode::Extend),
        )
    })
    .inner
}
fn ready(ui: &mut egui::Ui, preview: &mut Preview) {
    let bounds = ui.max_rect();
    let toolbar = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 33.));
    ui.painter()
        .rect_filled(toolbar, 0., Color32::from_gray(60));
    let at =
        |x, y, w, h| egui::Rect::from_min_size(bounds.min + egui::vec2(x, y), egui::vec2(w, h));
    if toolbar_button(
        ui,
        at(10., 6., 58., 21.),
        "◀ 上一张",
        preview.index > 0,
        false,
    )
    .clicked()
    {
        preview.select(preview.index - 1);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    if toolbar_button(
        ui,
        at(76., 6., 58., 21.),
        "下一张 ▶",
        preview.index + 1 < preview.items.len(),
        false,
    )
    .clicked()
    {
        preview.select(preview.index + 1);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    let right = bounds.width();
    if toolbar_button(ui, at(right - 48., 6., 38., 21.), "删除", true, true).clicked() {
        preview.confirm_delete = true;
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    if toolbar_button(ui, at(right - 94., 6., 38., 21.), "下载", true, false).clicked()
        && let Some(path) = rfd::FileDialog::new()
            .set_title("保存文件")
            .set_file_name(&preview.items[preview.index].name)
            .save_file()
    {
        preview.action = Some(PreviewAction::Download(path));
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    ui.painter()
        .rect_filled(at(right - 108., 6., 3., 21.), 0., Color32::from_gray(204));
    if toolbar_button(ui, at(right - 148., 6., 32., 21.), "1:1", true, false).clicked() {
        preview.scale = 1.;
        preview.initial_fit = false;
        preview.translation = egui::Vec2::ZERO;
    }
    if toolbar_button(ui, at(right - 216., 6., 60., 21.), "适配窗口", true, false).clicked() {
        let canvas = egui::vec2(bounds.width(), bounds.height() - 33.);
        preview.scale = ((canvas.x - 20.) / preview.dimensions.x)
            .min((canvas.y - 20.) / preview.dimensions.y)
            .clamp(0.001, 1.);
        preview.initial_fit = false;
        preview.translation = egui::Vec2::ZERO;
    }
    let Some(texture) = preview.texture.clone() else {
        return;
    };
    if preview.initial_fit {
        let size = ui
            .input(|input| input.viewport().outer_rect.map(|rect| rect.size()))
            .unwrap_or(egui::vec2(800., 600.));
        preview.scale = ((size.x - 20.) / preview.dimensions.x)
            .min((size.y - 100.) / preview.dimensions.y)
            .clamp(0.001, 1.);
        preview.initial_fit = false;
    }
    let canvas = egui::Rect::from_min_max(egui::pos2(bounds.left(), toolbar.bottom()), bounds.max);
    let response = ui.interact(
        canvas,
        ui.make_persistent_id("image-pan"),
        egui::Sense::drag(),
    );
    if response.dragged() {
        preview.translation += response.drag_delta();
    }
    if ui.is_enabled() && response.hovered() {
        let wheels = ui.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::MouseWheel { delta, .. } = event {
                        Some(delta.y)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for wheel in wheels {
            preview.zoom(wheel);
        }
    }
    ui.painter().with_clip_rect(canvas).image(
        texture.id(),
        egui::Rect::from_center_size(
            canvas.center() + preview.translation,
            preview.dimensions * preview.scale,
        ),
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
        Color32::WHITE,
    );
    if ui.is_enabled()
        && ui.input(|input| input.key_pressed(egui::Key::ArrowLeft))
        && preview.index > 0
    {
        preview.select(preview.index - 1);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    } else if ui.is_enabled()
        && ui.input(|input| input.key_pressed(egui::Key::ArrowRight))
        && preview.index + 1 < preview.items.len()
    {
        preview.select(preview.index + 1);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
}
fn confirm(ctx: &egui::Context, preview: &mut Preview) {
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(("image-delete", &preview.id)),
        egui::ViewportBuilder::default()
            .with_title("删除确认")
            .with_inner_size([380., 130.])
            .with_resizable(false),
        |ui, _| {
            ui.painter()
                .rect_filled(ui.max_rect(), 0., Color32::from_gray(244));
            egui::Frame::new().inner_margin(15).show(ui, |ui| {
                ui.label(format!(
                    "确定要删除文件 \"{}\" 吗？",
                    preview.items[preview.index].name
                ));
                ui.add_space(20.);
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() {
                        preview.action = Some(PreviewAction::Delete);
                        preview.confirm_delete = false;
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                    if ui.button("取消").clicked() {
                        preview.confirm_delete = false;
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
            });
            if ui.input(|input| {
                input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
            }) {
                preview.confirm_delete = false;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}
fn notice(ctx: &egui::Context, preview: &mut Preview) {
    let Some((title, text)) = preview.notice.clone() else {
        return;
    };
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(("image-notice", &preview.id)),
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([420., 150.])
            .with_resizable(false),
        |ui, _| {
            ui.painter()
                .rect_filled(ui.max_rect(), 0., Color32::from_gray(244));
            egui::Frame::new().inner_margin(15).show(ui, |ui| {
                ui.label(&text);
                ui.add_space(20.);
                if ui.button("确定").clicked() {
                    preview.notice = None;
                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                }
            });
            if ui.input(|input| {
                input.viewport().close_requested() || input.key_pressed(egui::Key::Enter)
            }) {
                preview.notice = None;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        },
    );
}
