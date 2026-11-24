//! Measured original TextInputDialog geometry, in an owned native child viewport.
use super::*;
use crate::tabs::query_results::view::Dialog;
pub(in crate::panels::sql) fn show(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    id: Id,
) -> (bool, bool) {
    let viewport = egui::ViewportId::from_hash_of((id, "custom-filter-window"));
    let capture = ctx
        .data(|data| data.get_temp::<bool>(Id::new("tomato-sql-filter-fixture")))
        .unwrap_or(false);
    let size = egui::vec2(352., 154.);
    let mut builder = egui::ViewportBuilder::default()
        .with_title("自定义筛选")
        .with_inner_size(size)
        .with_resizable(false);
    if dialog.focus
        && let Some(rect) = ctx.input(|input| input.viewport().inner_rect)
    {
        builder = builder.with_position(rect.center() - size / 2.);
    }
    if capture {
        builder = builder.with_active(false);
        ctx.data_mut(|data| data.insert_temp(Id::new("tomato-capture-viewport"), viewport));
    }
    ctx.show_viewport_immediate(viewport,builder,|ui,_| {
        let origin=ui.max_rect().min;
        let at=|x,y,w,h|Rect::from_min_size(origin+egui::vec2(x,y),egui::vec2(w,h));
        ui.painter().rect_filled(ui.max_rect(),0.,Color32::from_gray(244));
        let header=at(0.,0.,352.,67.);let mut mesh=egui::Mesh::default();
        for (pos,color) in [(header.left_top(),Color32::from_gray(244)),(header.right_top(),Color32::from_gray(244)),(header.right_bottom(),Color32::from_gray(249)),(header.left_bottom(),Color32::from_gray(249))] {mesh.vertices.push(egui::epaint::Vertex {pos,uv:egui::epaint::WHITE_UV,color});}mesh.indices.extend_from_slice(&[0,1,2,0,2,3]);ui.painter().add(mesh);
        ui.painter().line_segment([origin+egui::vec2(0.,67.5),origin+egui::vec2(352.,67.5)],Stroke::new(1.,Color32::from_gray(200)));
        ui.painter().text(origin+egui::vec2(10.,34.),Align2::LEFT_CENTER,"输入筛选条件",FontId::proportional(14.),Color32::BLACK);
        let icon_key=Id::new("original-filter-confirmation-icon");
        let texture=ctx.data(|data|data.get_temp::<egui::TextureHandle>(icon_key)).or_else(|| {
            let bytes=crate::asset_bytes::bytes("upstream/openjfx/dialog-confirm.png")?;
            let pixels=image::load_from_memory(bytes).ok()?.into_rgba8();
            let texture=ctx.load_texture("original-filter-confirmation",egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize,pixels.height() as usize],pixels.as_raw()),egui::TextureOptions::LINEAR);
            ctx.data_mut(|data|data.insert_temp(icon_key,texture.clone()));Some(texture)
        });
        if let Some(texture)=texture {ui.painter().image(texture.id(),at(294.,10.,48.,48.),Rect::from_min_max(Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE);}
        ui.style_mut().override_font_id=Some(FontId::proportional(12.));
        ui.visuals_mut().selection.bg_fill=Color32::from_rgb(0,150,201);
        ui.visuals_mut().selection.stroke=Stroke::new(1.,Color32::WHITE);
        ui.painter().text(origin+egui::vec2(10.,89.5),Align2::LEFT_CENTER,"支持 =、!=、~、!~、<、>：",FontId::proportional(12.),Color32::BLACK);
        let editor_id=dialog.editor.prepare_id(ctx,id.with("filter-input"));
        let output=ui.scope_builder(egui::UiBuilder::new().max_rect(at(181.,78.,161.,23.)),|ui| {
            egui::TextEdit::singleline(&mut dialog.input).id(editor_id).desired_width(161.).min_size(egui::vec2(161.,23.)).margin(egui::vec2(6.,3.)).char_limit(1024*1024).show(ui)
        }).inner;
        if dialog.focus {output.response.request_focus();let mut state=output.state;state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0),egui::text::CCursor::new(dialog.input.chars().count()))));state.store(ctx,editor_id);dialog.focus=false;}
        if ctx.memory(|memory|memory.focused())==Some(editor_id){ui.painter().rect_stroke(output.response.rect.expand(0.5),2.,Stroke::new(1.,Color32::from_rgb(3,158,211)),egui::StrokeKind::Inside);}
        let ok=crate::native_dialog::button(ui,at(182.,121.,75.,23.),"确定",true);
        let cancel=crate::native_dialog::button(ui,at(267.,121.,75.,23.),"取消",false);
        let canceled=cancel.clicked() || ui.input(|input|input.viewport().close_requested()||input.key_pressed(Key::Escape));
        let accepted=!canceled && (ok.clicked()||ui.input(|input|input.key_pressed(Key::Enter)));
        dialog.editor.trim(ctx);
        if capture {
            if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==viewport{Some(image.clone())}else{None})){ctx.data_mut(|data|data.insert_temp(Id::new("tomato-capture-image"),image));}
            if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from){let _=std::fs::write(path.with_extension("dialog.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"inputX":output.response.rect.left()-origin.x,"inputY":output.response.rect.top()-origin.y,"inputWidth":output.response.rect.width(),"inputHeight":output.response.rect.height(),"okX":ok.rect.left()-origin.x,"okY":ok.rect.top()-origin.y,"systemClipboardWritten":false}).to_string());}
        }
        if accepted||canceled{ctx.send_viewport_cmd(egui::ViewportCommand::Close);ctx.request_repaint_of(egui::ViewportId::ROOT);}
        (accepted,canceled)
    })
}
