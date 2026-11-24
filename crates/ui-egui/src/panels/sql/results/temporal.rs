//! Native auto-hiding calendar/time popup, anchored below the active cell field.
use super::*;
use crate::tabs::query_results::{temporal::Picker, temporal_model::Kind};
#[cfg(test)]
#[path = "temporal_alignment_tests.rs"]
mod alignment_tests;
pub(in crate::panels) fn picker_button(
    ui: &mut Ui,
    rect: Rect,
    kind: Kind,
    id: Id,
) -> egui::Response {
    #[derive(Clone)]
    struct Icon {
        scale: f32,
        texture: egui::TextureHandle,
    }
    let response = ui
        .interact(rect, id, Sense::CLICK)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("选择{}", kind.label()));
    ui.painter().rect_filled(rect, 0., Color32::from_gray(245));
    ui.painter().line_segment(
        [
            rect.left_top() + egui::vec2(0.5, 0.),
            rect.left_bottom() + egui::vec2(0.5, 0.),
        ],
        Stroke::new(1., Color32::from_gray(204)),
    );
    let key = Id::new("original-query-calendar-icon");
    let scale = ui.ctx().pixels_per_point();
    let icon=ui.ctx().data(|data|data.get_temp::<Icon>(key)).filter(|icon|icon.scale==scale).or_else(||{
        let svg=b"<svg xmlns='http://www.w3.org/2000/svg' width='12.4' height='12.4' viewBox='2 1 20 20'><path fill='#555' d='M20 3h-1V1h-2v2H7V1H5v2H4c-1.11 0-1.99.9-1.99 2L2 19c0 1.1.88 2 2 2h16c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H4V8h16v11z'/></svg>";
        let tree=resvg::usvg::Tree::from_data(svg,&resvg::usvg::Options::default()).ok()?;let size=(12.4*scale).ceil() as u32;let mut pixels=resvg::tiny_skia::Pixmap::new(size,size)?;
        resvg::render(&tree,resvg::tiny_skia::Transform::from_scale(scale,scale),&mut pixels.as_mut());
        let texture=ui.ctx().load_texture("original-query-calendar",egui::ColorImage::from_rgba_premultiplied([size as usize,size as usize],pixels.data()),egui::TextureOptions::LINEAR);
        let icon=Icon {scale,texture};ui.ctx().data_mut(|data|data.insert_temp(key,icon.clone()));Some(icon)
    });
    if let Some(icon) = icon {
        ui.painter().image(
            icon.texture.id(),
            Rect::from_center_size(rect.center(), egui::vec2(12.4, 12.4)),
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("选择{}", kind.label()),
        )
    });
    response
}
pub(super) fn button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    colors: (Color32, Color32, Color32),
    font: FontId,
    id: Id,
) -> egui::Response {
    let (fill, border, text) = colors;
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().rect_filled(
        rect,
        if border == Color32::TRANSPARENT {
            3.
        } else {
            0.
        },
        fill,
    );
    ui.painter()
        .rect_stroke(rect, 0., Stroke::new(1., border), egui::StrokeKind::Inside);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}
pub(super) fn show(ctx: &egui::Context, result: &QueryResult, state: &mut State, id: Id) {
    let Some(editing) = &mut state.editing else {
        return;
    };
    let Some(editor) = &mut editing.editor else {
        return;
    };
    let Some(mut picker) = editor.temporal.take() else {
        return;
    };
    let (accept, close) = show_picker(ctx, &mut picker, id);
    if accept {
        for index in 0..3 {
            picker.commit_time(index);
        }
        editor.text = picker.value.formatted();
        editor.null = false;
        let original = editing.cells.get(&(editor.row, editor.column)).map_or_else(
            || result.value(editor.row, editor.column),
            |value| value.as_deref(),
        );
        editor.changed = original != Some(editor.text.as_str());
        editor.focus = true;
    }
    if accept || close {
        picker.release(ctx);
    } else {
        editor.temporal = Some(picker);
    }
}
pub(in crate::panels) fn show_picker(
    ctx: &egui::Context,
    picker: &mut Picker,
    id: Id,
) -> (bool, bool) {
    let viewport = egui::ViewportId::from_hash_of((id, "temporal-picker"));
    let capture = ctx
        .data(|data| data.get_temp::<bool>(Id::new("tomato-sql-temporal-fixture")))
        .unwrap_or(false);
    let mut builder = egui::ViewportBuilder::default()
        .with_title("日期时间选择")
        .with_inner_size(egui::vec2(260., picker.height()))
        .with_position(picker.anchor)
        .with_resizable(false)
        .with_decorations(false)
        .with_taskbar(false)
        .with_always_on_top();
    if capture {
        builder = builder.with_active(false);
        ctx.data_mut(|data| data.insert_temp(Id::new("tomato-capture-viewport"), viewport));
    }
    let (accept,close)=ctx.show_viewport_immediate(viewport,builder,|ui,_| {
        let rect=ui.max_rect();let origin=rect.min;let at=|x,y,w,h|Rect::from_min_size(origin+egui::vec2(x,y),egui::vec2(w,h));
        ui.painter().rect_filled(rect,0.,Color32::WHITE);ui.painter().rect_stroke(rect,0.,Stroke::new(1.,Color32::from_gray(153)),egui::StrokeKind::Inside);
        let family=if ctx.fonts(|fonts|fonts.definitions().families.contains_key(&egui::FontFamily::Name("table".into()))){egui::FontFamily::Name("table".into())}else{egui::FontFamily::Proportional};
        let font=FontId::new(12.,family.clone());
        let time_y=if picker.value.kind==Kind::Time{11.}else{calendar(ui,picker,origin,id,&font)+8.};
        if picker.value.kind!=Kind::Date {
            for (index,label) in ["时","分","秒"].iter().enumerate() {
                let x=11.+index as f32*76.;ui.painter().text(origin+egui::vec2(x,time_y+15.),Align2::LEFT_CENTER,*label,font.clone(),Color32::BLACK);
                spinner(ui,picker,index,at(x+16.,time_y,56.,30.),id,FontId::new(13.,family.clone()));
            }
        }
        let confirm=button(ui,at(209.,rect.height()-34.,40.,23.),"确定",(Color32::from_rgb(53,146,203),Color32::TRANSPARENT,Color32::WHITE),font,id.with("temporal-apply"));
        let focused=ui.input(|input|input.focused);let unfocused=picker.focused_once&&!focused;picker.focused_once|=focused;
        let close=ui.input(|input|input.viewport().close_requested()||input.key_pressed(Key::Escape))||(!capture&&unfocused);
        let accept=!close&&confirm.clicked();
        if accept||close{ctx.send_viewport_cmd(egui::ViewportCommand::Close);ctx.request_repaint_of(egui::ViewportId::ROOT);}
        if capture {
            if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==viewport{Some(image.clone())}else{None})){ctx.data_mut(|data|data.insert_temp(Id::new("tomato-capture-image"),image));}
            if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from){let _=std::fs::write(path.with_extension("temporal.json"),serde_json::json!({"ready":true,"clientWidth":rect.width(),"clientHeight":rect.height(),"kind":picker.value.kind.label(),"date":picker.value.selected.to_string(),"time":picker.value.time,"okX":confirm.rect.left()-origin.x,"okY":confirm.rect.top()-origin.y,"systemClipboardWritten":false}).to_string());}
        }
        (accept,close)
    });
    (accept, close)
}
fn calendar(ui: &mut Ui, picker: &mut Picker, origin: Pos2, id: Id, font: &FontId) -> f32 {
    let month = format!(
        "{}-{:02}",
        picker.value.cursor.year, picker.value.cursor.month
    );
    let bold = if ui.ctx().fonts(|fonts| {
        fonts
            .definitions()
            .families
            .contains_key(&egui::FontFamily::Name("table-bold".into()))
    }) {
        FontId::new(12., egui::FontFamily::Name("table-bold".into()))
    } else {
        font.clone()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(month, bold, Color32::from_gray(51));
    let width = galley.size().x.max(50.);
    let left = (260. - (23. + 8. + width + 8. + 23.)) / 2.;
    let at = |x, y, w, h| Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
    for (label, x, offset) in [("<", left, -1), (">", left + 23. + 8. + width + 8., 1)] {
        if button(
            ui,
            at(x, 11., 23., 17.),
            label,
            (
                Color32::from_gray(240),
                Color32::from_gray(204),
                Color32::BLACK,
            ),
            font.clone(),
            id.with(("month", offset)),
        )
        .clicked()
            && let Some(month) = picker.value.cursor.shift_month(offset)
        {
            picker.value.cursor = month;
        }
    }
    let heading = at(left + 31., 11., width, 17.);
    ui.painter().galley(
        heading.center() - galley.size() * 0.5,
        galley,
        Color32::from_gray(51),
    );
    for (column, label) in ["日", "一", "二", "三", "四", "五", "六"]
        .iter()
        .enumerate()
    {
        ui.painter().text(
            origin + egui::vec2(34. + column as f32 * 32., 39.5),
            Align2::CENTER_CENTER,
            *label,
            font.clone(),
            Color32::from_gray(136),
        );
    }
    let start = picker.value.cursor.weekday();
    let days = picker.value.cursor.days();
    for day in 1..=days {
        let index = start + day - 1;
        let date = crate::tabs::query_results::temporal_model::Date {
            day,
            ..picker.value.cursor
        };
        let selected = date == picker.value.selected;
        if button(
            ui,
            at(
                19. + (index % 7) as f32 * 32.,
                49. + (index / 7) as f32 * 27.,
                30.,
                25.,
            ),
            &day.to_string(),
            (
                if selected {
                    Color32::from_rgb(53, 146, 203)
                } else {
                    Color32::WHITE
                },
                if selected {
                    Color32::from_rgb(53, 146, 203)
                } else {
                    Color32::from_gray(224)
                },
                if selected {
                    Color32::WHITE
                } else {
                    Color32::from_gray(51)
                },
            ),
            FontId::new(11., font.family.clone()),
            id.with(("day", day)),
        )
        .clicked()
        {
            picker.value.selected = date;
        }
    }
    11. + 36. + (start + days).div_ceil(7) as f32 * 27.
}
fn spinner(ui: &mut Ui, picker: &mut Picker, index: usize, rect: Rect, id: Id, font: FontId) {
    let field = Rect::from_min_max(rect.min, rect.right_bottom() - egui::vec2(24., 0.));
    ui.painter().rect_filled(rect, 3., Color32::WHITE);
    ui.painter().rect_stroke(
        rect,
        3.,
        Stroke::new(1., Color32::from_gray(181)),
        egui::StrokeKind::Inside,
    );
    let field_id = picker.editors[index].prepare_id(ui.ctx(), id.with(("time-field", index)));
    let enter = ui.input(|input| input.key_pressed(Key::Enter));
    let had_focus = ui.memory(|memory| memory.focused()) == Some(field_id);
    let keyboard = ui.input(|input| {
        i32::from(input.key_pressed(Key::ArrowUp)) - i32::from(input.key_pressed(Key::ArrowDown))
    });
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(field.shrink(1.)), |ui| {
            egui::TextEdit::singleline(&mut picker.fields[index])
                .id(field_id)
                .font(font)
                .frame(egui::Frame::NONE)
                .char_limit(64)
                .horizontal_align(egui::Align::Center)
                .vertical_align(egui::Align::Center)
                .margin(egui::vec2(2., 0.))
                .desired_width(field.width() - 2.)
                .min_size(field.shrink(1.).size())
                .show(ui)
                .response
        })
        .inner;
    if response.lost_focus() || (enter && had_focus) {
        picker.commit_time(index);
    }
    if enter && had_focus {
        response.request_focus();
    }
    if let Some(state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), field_id) {
        state.trim_undo_memory(64 * 1024);
    }
    let focused = ui.memory(|memory| memory.focused());
    if keyboard != 0
        && (focused == Some(field_id)
            || [0, 1]
                .into_iter()
                .any(|part| focused == Some(id.with(("spinner", index, part)))))
    {
        step(picker, index, keyboard);
    }
    for (part, delta) in [(0, 1), (1, -1)] {
        let arrow = Rect::from_min_size(
            egui::pos2(field.right(), rect.top() + part as f32 * 15.),
            egui::vec2(24., 15.),
        );
        let arrow_id = id.with(("spinner", index, part));
        let response = ui
            .interact(arrow, arrow_id, Sense::CLICK)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        crate::native_dialog::gradient(
            ui,
            arrow,
            Color32::from_gray(if part == 0 { 253 } else { 249 }),
            Color32::from_gray(if part == 0 { 219 } else { 226 }),
        );
        ui.painter().rect_stroke(
            arrow,
            0.,
            Stroke::new(1., Color32::from_gray(181)),
            egui::StrokeKind::Inside,
        );
        if repeat(ui, &response, arrow_id) {
            step(picker, index, delta);
        }
        let center = arrow.center();
        let sign = if part == 0 { -1. } else { 1. };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(-3.5, -sign * 1.5),
                center + egui::vec2(3.5, -sign * 1.5),
                center + egui::vec2(0., sign * 2.5),
            ],
            Color32::from_gray(85),
            Stroke::NONE,
        ));
    }
}
fn step(picker: &mut Picker, index: usize, delta: i32) {
    picker.commit_time(index);
    picker.value.time[index] =
        (picker.value.time[index] as i32 + delta).clamp(0, if index == 0 { 23 } else { 59 }) as u32;
    picker.fields[index] = picker.value.time[index].to_string();
}
fn repeat(ui: &Ui, response: &egui::Response, id: Id) -> bool {
    crate::native_dialog::spinner::repeat(ui, response, id)
}

/// Shared original calendar and time controls for MQ query bounds.
pub(in crate::panels) fn datetime_field(
    ui: &mut Ui,
    text: &mut String,
    state: &mut Option<Picker>,
    width: f32,
    salt: &str,
) {
    let id = ui.id().with(salt);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.), Sense::hover());
    let field = Rect::from_min_max(rect.min, rect.max - egui::vec2(22., 0.));
    ui.put(
        field,
        egui::TextEdit::singleline(text)
            .font(FontId::proportional(12.))
            .vertical_align(egui::Align::Center),
    );
    let icon = Rect::from_min_max(egui::pos2(field.right(), rect.top()), rect.max);
    if picker_button(ui, icon, Kind::DateTime, id.with("open")).clicked() {
        *state = Some(Picker::new(Kind::DateTime, text, rect.left_bottom()));
    }
    let Some(mut picker) = state.take() else {
        return;
    };
    let mut accept = false;
    let output = egui::Window::new("日期时间选择")
        .id(id.with("popup"))
        .title_bar(false)
        .resizable(false)
        .fixed_pos(picker.anchor)
        .fixed_size([260., picker.height()])
        .show(ui.ctx(), |ui| {
            let origin = ui.cursor().min;
            let font = FontId::proportional(12.);
            let y = calendar(ui, &mut picker, origin, id, &font) + 8.;
            for (index, label) in ["时", "分", "秒"].iter().enumerate() {
                let x = 11. + index as f32 * 76.;
                ui.painter().text(
                    origin + egui::vec2(x, y + 15.),
                    Align2::LEFT_CENTER,
                    *label,
                    font.clone(),
                    Color32::BLACK,
                );
                spinner(
                    ui,
                    &mut picker,
                    index,
                    Rect::from_min_size(origin + egui::vec2(x + 16., y), egui::vec2(56., 30.)),
                    id,
                    font.clone(),
                );
            }
            let confirm =
                Rect::from_min_size(origin + egui::vec2(209., y + 38.), egui::vec2(40., 23.));
            accept = button(
                ui,
                confirm,
                "确定",
                (
                    Color32::from_rgb(53, 146, 203),
                    Color32::TRANSPARENT,
                    Color32::WHITE,
                ),
                font,
                id.with("apply"),
            )
            .clicked();
            ui.allocate_rect(
                Rect::from_min_size(origin, egui::vec2(260., y + 65.)),
                Sense::hover(),
            );
        });
    let close = ui.input(|i| {
        i.key_pressed(Key::Escape)
            || (i.pointer.any_click()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !rect.contains(p)
                        && output
                            .as_ref()
                            .is_some_and(|o| !o.response.rect.contains(p))
                }))
    });
    if accept {
        for index in 0..3 {
            picker.commit_time(index);
        }
        *text = picker.value.formatted();
    }
    if accept || close {
        picker.release(ui.ctx());
    } else {
        *state = Some(picker);
    }
}
