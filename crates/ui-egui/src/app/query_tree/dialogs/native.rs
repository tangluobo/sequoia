use super::*;
use crate::native_dialog::button;
pub(super) fn show(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    title: &str,
    label: &str,
    icons: &mut crate::icons::Icons,
) -> (bool, bool) {
    let deletion = matches!(dialog.kind, Kind::Delete(_));
    let size = if deletion {
        let directory =
            matches!(&dialog.kind,Kind::Delete(entries) if entries.len()==1&&entries[0].directory);
        egui::vec2(if directory { 361. } else { 360. }, 142.)
    } else if matches!(dialog.kind, Kind::Rename(_)) {
        egui::vec2(297., 101.)
    } else {
        egui::vec2(309., 101.)
    };
    let id = egui::ViewportId::from_hash_of("named-query-dialog");
    let capture = ctx
        .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-query-dialog-fixture")))
        .unwrap_or_default();
    let mut builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_resizable(false);
    if dialog.focus
        && let Some(rect) = ctx.input(|input| input.viewport().inner_rect)
    {
        builder = builder.with_position(rect.center() - size / 2.);
    }
    if capture {
        builder = builder.with_active(false);
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
    }
    ctx.show_viewport_immediate(id,builder,|ui,_|{
        let origin=ui.max_rect().min;
        ui.painter().rect_filled(ui.max_rect(),0.,Color32::from_gray(244));
        ui.style_mut().override_font_id=Some(egui::FontId::proportional(12.));
        ui.spacing_mut().interact_size.y=23.;
        ui.spacing_mut().button_padding=egui::vec2(4.,3.);
        ui.visuals_mut().selection.bg_fill=Color32::from_rgb(0,150,201);
        ui.visuals_mut().selection.stroke=egui::Stroke::new(1.,Color32::WHITE);
        let at=|x,y,w,h|egui::Rect::from_min_size(origin+egui::vec2(x,y),egui::vec2(w,h));
        icons.paint(ui,"upstream/openjfx/dialog-confirm.png",at(if deletion{size.x-58.}else{10.},10.,48.,48.));
        let mut input_rect=None;
        if let Kind::Delete(entries)=&dialog.kind{
            let message=if entries.len()==1 {format!("确定要删除{} \"{}\" {}吗？",if entries[0].directory{"目录"}else{"查询"},entries[0].name,if entries[0].directory{"及其所有内容"}else{""})}else{format!("确定要删除选中的 {} 个查询项目吗？",entries.len())};
            ui.put(at(10.,18.,size.x-78.,34.),egui::Label::new(message).wrap());
            ui.painter().line_segment([origin+egui::vec2(0.,67.),origin+egui::vec2(size.x,67.)],egui::Stroke::new(1.,Color32::from_gray(200)));
        }else{
            let input_x=size.x-171.;
            ui.painter().text(origin+egui::vec2(68.,29.5),egui::Align2::LEFT_CENTER,label,egui::FontId::proportional(12.),Color32::BLACK);
            let mut output=None;
            let editor_id=dialog.editor.prepare_id(ui.ctx(),egui::Id::new("named-query-name"));
            ui.scope_builder(egui::UiBuilder::new().max_rect(at(input_x,18.,161.,23.)),|ui|{
                output=Some(egui::TextEdit::singleline(&mut dialog.text).id(editor_id).desired_width(161.).min_size(egui::vec2(161.,23.)).margin(egui::vec2(6.,3.)).show(ui));
            });
            let output=output.unwrap();
            input_rect=Some(output.response.rect);
            if dialog.focus{
                output.response.request_focus();let mut state=output.state;
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0),egui::text::CCursor::new(dialog.text.chars().count()))));
                state.store(ui.ctx(),output.response.id);dialog.focus=false;
            }
        }
        let ok=button(ui,at(size.x-170.,size.y-33.,75.,23.),"确定",true);
        let cancel=button(ui,at(size.x-85.,size.y-33.,75.,23.),"取消",false);
        let mut accepted=ok.clicked();
        let mut canceled=cancel.clicked();
        canceled|=ui.input(|input|input.viewport().close_requested()||input.key_pressed(egui::Key::Escape));
        accepted|=!canceled&&ui.input(|input|input.key_pressed(egui::Key::Enter));
        if capture{
            if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==id{Some(image.clone())}else{None})){ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));}
            if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from){let _=std::fs::write(path.with_extension("state.json"),serde_json::json!({"ready":true,"title":title,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"inputWidth":input_rect.map(|rect|rect.width()),"inputHeight":input_rect.map(|rect|rect.height()),"inputX":input_rect.map(|rect|rect.left()-origin.x),"inputY":input_rect.map(|rect|rect.top()-origin.y),"okX":ok.rect.left()-origin.x,"okY":ok.rect.top()-origin.y,"buttonWidth":ok.rect.width(),"buttonHeight":ok.rect.height()}).to_string());}
        }
        if accepted||canceled{ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);ui.ctx().request_repaint_of(egui::ViewportId::ROOT);}
        (accepted,canceled)
    })
}
