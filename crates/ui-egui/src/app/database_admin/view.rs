//! Geometry and tab/button states measured from the unchanged Java dialogs.
use super::*;
use egui::{Rect, vec2};
pub(super) enum Action {
    Close,
    Submit,
}
fn label(ui: &Ui, rect: Rect, text: &str, size: f32) {
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        crate::native_dialog::font(ui.ctx(), size),
        Color32::BLACK,
    );
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft) -> Option<Action> {
    let bounds = ui.max_rect();
    let origin = bounds.min;
    let w = bounds.width();
    let h = bounds.height();
    let at = |x, y, width, height| Rect::from_min_size(origin + vec2(x, y), vec2(width, height));
    let mut action = None;
    ui.painter()
        .rect_filled(bounds, 0., Color32::from_gray(244));
    ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 12.));
    if draft.mode == Mode::Drop {
        if let Some(error) = &draft.error {
            ui.put(
                at(30., 35., w - 60., h - 100.),
                egui::Label::new(error).wrap(),
            );
        }
        if ui
            .add_enabled_ui(!draft.running, |ui| {
                crate::native_dialog::button(ui, at(w - 95., h - 38., 80., 23.), "关闭", false)
                    .clicked()
            })
            .inner
        {
            action = Some(Action::Close);
        }
        if ui.input(|i| i.viewport().close_requested()) {
            if draft.running {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else {
                action = Some(Action::Close);
            }
        }
        return action;
    }
    ui.painter()
        .rect_filled(at(15., 15., w - 30., 37.), 0., Color32::from_gray(236));
    for (index, (text, x, width)) in [("常规", 15., 49.), ("SQL预览", 64., 73.)]
        .into_iter()
        .enumerate()
    {
        let rect = at(x, 15., width, 37.);
        let response = ui.interact(rect, ui.id().with(text), egui::Sense::click());
        ui.painter().rect_filled(
            rect,
            0.,
            Color32::from_gray(if index == draft.page { 255 } else { 226 }),
        );
        ui.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            egui::Stroke::new(1., Color32::from_gray(192)),
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::from_gray(51),
        );
        if response.clicked() {
            draft.page = index;
        }
    }
    let ready = !draft.loading && !draft.running;
    let shift = if draft.mode == Mode::Edit { -5. } else { 0. };
    if draft.page == 0 {
        label(ui, at(15., 52., w - 30., 15.), "数据库名称：", 12.);
        if draft.mode == Mode::Edit {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(at(15., 79., w - 30., 18.))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&draft.database)
                                .font(crate::native_dialog::font(ui.ctx(), 13.))
                                .strong(),
                        )
                        .truncate(),
                    );
                },
            );
        } else {
            let id = draft.editors[0].prepare_id(ui.ctx(), ui.id().with("database-name"));
            let response = ui
                .add_enabled_ui(ready, |ui| {
                    ui.put(
                        at(15., 79., w - 30., 23.),
                        TextEdit::singleline(&mut draft.database)
                            .id(id)
                            .hint_text("请输入数据库名称")
                            .font(crate::native_dialog::font(ui.ctx(), 12.))
                            .char_limit(64),
                    )
                })
                .inner;
            if draft.focus {
                response.request_focus();
                draft.focus = false;
            }
            draft.editors[0].trim(ui.ctx());
        }
        label(ui, at(15., 114. + shift, w - 30., 15.), "字符集：", 12.);
        let mut charset = draft.charset.clone();
        let display = if charset.is_empty() {
            "(默认)".into()
        } else {
            charset.clone()
        };
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(at(15., 141. + shift, w - 30., 28.)),
            |ui| {
                ui.spacing_mut().interact_size.y = 28.;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::WHITE;
                ui.visuals_mut().widgets.inactive.bg_fill = Color32::WHITE;
                ui.visuals_mut().widgets.inactive.fg_stroke.color = Color32::from_gray(102);
                ui.add_enabled_ui(ready, |ui| {
                    egui::ComboBox::from_id_salt("database-charset")
                        .width(w - 30.)
                        .selected_text(display)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut charset, String::new(), "(默认)");
                            for (name, _) in &draft.info.charsets {
                                ui.selectable_value(&mut charset, name.clone(), name);
                            }
                        });
                });
            },
        );
        if charset != draft.charset {
            draft.charset = charset;
            draft.collation.clear();
        }
        label(ui, at(15., 181. + shift, w - 30., 15.), "排序规则：", 12.);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(at(15., 208. + shift, w - 30., 28.)),
            |ui| {
                ui.spacing_mut().interact_size.y = 28.;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::WHITE;
                ui.visuals_mut().widgets.inactive.bg_fill = Color32::WHITE;
                ui.visuals_mut().widgets.inactive.fg_stroke.color = Color32::from_gray(102);
                ui.add_enabled_ui(ready, |ui| {
                    egui::ComboBox::from_id_salt("database-collation")
                        .width(w - 30.)
                        .selected_text(if draft.collation.is_empty() {
                            "(默认)"
                        } else {
                            &draft.collation
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.collation, String::new(), "(默认)");
                            if let Some((_, values)) = draft
                                .info
                                .charsets
                                .iter()
                                .find(|(name, _)| name == &draft.charset)
                            {
                                for value in values {
                                    ui.selectable_value(&mut draft.collation, value.clone(), value);
                                }
                            }
                        });
                });
            },
        );
    } else {
        let sql = draft.preview();
        let mut readonly = sql.as_str();
        let id = draft.editors[1].prepare_id(ui.ctx(), ui.id().with("database-sql"));
        let response = ui.put(
            at(15., 52., w - 30., 36.),
            TextEdit::multiline(&mut readonly)
                .id(id)
                .font(egui::FontId::monospace(13.))
                .layouter(&mut crate::panels::sql_preview_layout),
        );
        response.context_menu(|ui| {
            if ui.button("复制").clicked() {
                ui.ctx().copy_text(sql.clone());
                ui.close();
            }
        });
        if response.has_focus()
            && ui.input(|i| {
                i.events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy))
            })
        {
            ui.ctx().copy_text(sql);
        }
        draft.editors[1].trim(ui.ctx());
    }
    if let Some(error) = &draft.error {
        ui.put(
            at(15., h - 53., w - 30., 15.),
            egui::Label::new(RichText::new(error).color(Color32::from_rgb(180, 30, 30))).truncate(),
        );
    }
    if ui
        .add_enabled_ui(!draft.running, |ui| {
            crate::native_dialog::button(ui, at(w - 185., h - 38., 80., 23.), "取消", false)
                .clicked()
        })
        .inner
    {
        action = Some(Action::Close);
    }
    let text = if draft.mode == Mode::Edit {
        "保存"
    } else {
        "确定"
    };
    let rect = at(w - 95., h - 38., 80., 23.);
    let response = ui
        .add_enabled_ui(ready, |ui| {
            ui.interact(rect, ui.id().with(text), egui::Sense::click())
        })
        .inner;
    ui.painter()
        .rect_filled(rect, 4., Color32::from_rgb(7, 193, 96));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::WHITE,
    );
    if response.clicked() {
        action = Some(Action::Submit);
    }
    if ui.input(|i| i.viewport().close_requested()) {
        if draft.running {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else {
            action = Some(Action::Close);
        }
    }
    ui.allocate_rect(bounds, egui::Sense::hover());
    action
}
