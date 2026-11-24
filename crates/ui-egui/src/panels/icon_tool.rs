use super::*;
use crate::tabs::icon_tool::State;
use tomato_services::tools::icon::job::{Action, Job, Output};
mod preview;
mod slider;

pub(super) fn render(
    ui: &mut Ui,
    tool: &mut ToolState,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    super::tools::header(
        ui,
        icons,
        "image_background_remover",
        "图标制作工具",
        "JPG/PNG → 自动去除纯色背景并生成透明PNG图标",
    );
    let state = tool.icon_tool.get_or_insert_with(Default::default);
    poll(ui, state);
    let body = ui.available_rect_before_wrap();
    let scrolling = state.content_height > body.height();
    let mut viewport = body;
    if scrolling {
        viewport.max.x -= 16.;
    }
    let mut changed = false;
    let mut save = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(viewport),|ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(viewport));
        let output=ScrollArea::vertical().auto_shrink([false,false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).vertical_scroll_offset(state.scroll_offset).show(ui,|ui| {
            egui::Frame::new().inner_margin(egui::Margin{left:25,right:25,top:20,bottom:25}).show(ui,|ui| {
                ui.spacing_mut().item_spacing.y=20.;
                section(ui,"转换说明",|ui| {
                    panel(ui,40.,|ui,rect| {super::tools::place_left(ui,rect.shrink2(egui::vec2(16.,8.)),egui::Label::new(RichText::new("自动识别图片四周占比最高的纯色背景，支持白色、黑色及彩色背景；颜色容差用于覆盖 JPG 压缩偏色").size(13.)).wrap());});
                });
                section(ui,"背景颜色容差",|ui| {panel(ui,59.,|ui,rect| {
                    changed|=slider::show(ui,egui::Rect::from_min_max(rect.min+egui::vec2(16.,8.),rect.max-egui::vec2(56.,8.)),&mut state.options.tolerance,(0,120,20),"tolerance");
                    text(ui,egui::pos2(rect.right()-30.,rect.center().y),egui::Align2::CENTER_CENTER,&state.options.tolerance.to_string(),12.,Color32::from_gray(85));
                });});
                section(ui,"输出选项",|ui| {
                    panel(ui,40.,|ui,rect| {changed|=super::tools::place_left(ui,rect.shrink2(egui::vec2(16.,8.)),egui::Checkbox::new(&mut state.options.auto_crop,RichText::new("自动裁切周边透明区域（按非透明像素最小包围矩形，批量生效）").size(13.))).changed();});
                    ui.add_space(8.);
                    panel(ui,59.,|ui,rect| {
                        changed|=super::tools::place_left(ui,egui::Rect::from_min_size(rect.min+egui::vec2(16.,18.),egui::vec2(195.,23.)),egui::Checkbox::new(&mut state.options.smooth,RichText::new("边沿平滑（抑制 JPG 毛边）").size(13.))).changed();
                        text(ui,rect.min+egui::vec2(211.,29.5),egui::Align2::LEFT_CENTER,"羽化半径:",12.,Color32::from_gray(85));
                        let slider=egui::Rect::from_min_max(rect.min+egui::vec2(277.,8.),rect.max-egui::vec2(56.,8.));
                        changed|=ui.add_enabled_ui(state.options.smooth,|ui| slider::show(ui,slider,&mut state.options.feather,(1,5,1),"feather")).inner;
                        text(ui,egui::pos2(rect.right()-30.,rect.center().y),egui::Align2::CENTER_CENTER,&state.options.feather.to_string(),12.,Color32::from_gray(85));
                    });
                });
                section(ui,"选择源文件/目录",|ui| {
                    let (rect,_)=ui.allocate_exact_size(egui::vec2(ui.available_width(),32.),egui::Sense::hover());
                    let input=egui::Rect::from_min_max(rect.min,rect.max-egui::vec2(184.,0.));
                    changed|=ui.place(input,TextEdit::singleline(&mut tool.source).font(egui::FontId::proportional(13.)).hint_text("选择 JPG/JPEG/PNG 文件或目录").margin(egui::vec2(8.,7.))).changed();
                    for (i,caption) in ["选择文件","浏览目录"].into_iter().enumerate() {
                        let button=egui::Rect::from_min_size(egui::pos2(rect.right()-176.+i as f32*92.,rect.top()),egui::vec2(84.,31.));
                        if ui.place(button,egui::Button::new(RichText::new(caption).size(13.).color(Color32::WHITE)).fill(Color32::from_rgb(76,175,80)).stroke(egui::Stroke::NONE).corner_radius(4)).clicked() {
                            let path=if i==0 {rfd::FileDialog::new().add_filter("图片",&["jpg","jpeg","png"]).pick_file()}else {rfd::FileDialog::new().pick_folder()};
                            if let Some(path)=path {state.directory=i==1;tool.source=path.display().to_string();state.original=None;state.processed=None;changed=true;}
                        }
                    }
                });
                if !state.directory && state.processed.is_some() {
                    section(ui,"预览（仅单文件模式）",|ui| {preview::show(ui,state,&mut save);});
                }else if ui.add_sized([116.,36.],button("批量保存",Color32::from_rgb(76,175,80))).clicked() {save=true;}
                if !state.status.is_empty() {ui.colored_label(if state.error {Color32::DARK_RED}else {Color32::from_gray(85)},&state.status);}
            });
        });
        state.scroll_offset=output.state.offset.y;
        state.content_height=output.content_size.y;
    });
    if scrolling {
        crate::native_scrollbar::show(
            ui,
            egui::Rect::from_min_max(egui::pos2(body.right() - 16., body.top()), body.max),
            true,
            &mut state.scroll_offset,
            crate::native_scrollbar::Metrics {
                extent: body.height(),
                total: state.content_height,
                unit: 40.,
            },
            ui.id().with("icon-scroll"),
            &mut state.scrollbar,
        );
    }
    if changed {
        state.options.margins = [0; 4];
        state.invalidate();
        state.error = false;
    }
    if save && state.job.is_none() {
        if tool.source.is_empty() {
            state.status = "请选择源文件或目录".into();
            state.error = true;
        } else if state.ready() {
            start(ui, state, &tool.source, Action::Save(state.source_hash));
            state.status = "正在保存…".into();
        } else {
            state.status = "请等待预览更新后保存".into();
        }
    }
    if state.job.is_none()
        && !tool.source.is_empty()
        && state
            .dirty_at
            .is_some_and(|at| at.elapsed() >= std::time::Duration::from_millis(300))
    {
        start(ui, state, &tool.source, Action::Preview);
    }
    if state.dirty_at.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(300));
    }
    None
}
fn start(ui: &Ui, state: &mut State, source: &str, action: Action) {
    let ctx = ui.ctx().clone();
    state.job_revision = state.revision;
    state.job = Some(Job::start(
        source.into(),
        state.options.clone(),
        action,
        move || ctx.request_repaint(),
    ));
    state.dirty_at = None;
}
fn poll(ui: &Ui, state: &mut State) {
    let Some(result) = state.job.as_ref().and_then(Job::poll) else {
        return;
    };
    state.job = None;
    if state.job_revision != state.revision {
        return;
    }
    state.error = false;
    match result {
        Ok(Output::Directory(count)) => {
            state.directory = true;
            state.original = None;
            state.processed = None;
            state.preview_revision = Some(state.revision);
            state.status = format!("找到 {count} 个图片文件");
        }
        Ok(Output::Preview(preview)) => {
            state.directory = false;
            state.sizes = [preview.original_size, preview.processed_size];
            state.source_hash = Some(preview.source_hash);
            for (slot, thumb) in [
                (&mut state.original, preview.original),
                (&mut state.processed, preview.processed),
            ] {
                *slot = Some(ui.ctx().load_texture(
                    "icon-preview",
                    egui::ColorImage::from_rgba_premultiplied(thumb.size, &thumb.rgba),
                    egui::TextureOptions::LINEAR,
                ));
            }
            state.crop.reset(state.sizes[1]);
            state.sync_fields();
            state.preview_revision = Some(state.revision);
            state.status.clear();
        }
        Ok(Output::Saved {
            total,
            success,
            failed,
        }) => {
            state.status = format!(
                "保存完成！总计: {total}, 成功: {success}, 失败: {failed}（已保存到源文件同目录，文件名加 _transparent）"
            );
            state.error = failed > 0;
        }
        Err(error) => {
            state.status = error.to_string();
            state.error = true;
        }
    }
}
fn text(ui: &Ui, pos: egui::Pos2, align: egui::Align2, value: &str, size: f32, color: Color32) {
    ui.painter()
        .text(pos, align, value, egui::FontId::proportional(size), color);
}
fn button(label: &str, color: Color32) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).size(14.).color(Color32::WHITE))
        .fill(color)
        .stroke(egui::Stroke::NONE)
        .corner_radius(4)
}
fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.), egui::Sense::hover());
        ui.painter().text(
            rect.min,
            egui::Align2::LEFT_TOP,
            title,
            egui::FontId::new(14., egui::FontFamily::Name("md-bold".into())),
            Color32::from_gray(51),
        );
        ui.add_space(8.);
        body(ui);
    });
}
fn panel(ui: &mut Ui, height: f32, body: impl FnOnce(&mut Ui, egui::Rect)) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter().rect(
        rect,
        4,
        Color32::from_rgb(248, 249, 250),
        egui::Stroke::new(1., Color32::from_gray(224)),
        egui::StrokeKind::Inside,
    );
    body(ui, rect);
}
