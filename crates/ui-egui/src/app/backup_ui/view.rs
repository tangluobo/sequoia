use super::draft::Draft;
use crate::icons::Icons;
use egui::{Align2, Color32, Rect, Ui, Vec2};

pub(super) enum Action {
    Start,
    ConfirmRestore,
    Close,
    SaveTemplate,
    LoadTemplate,
    ClearTemplate,
    RestoreSource(super::restore_source::Choice),
}
pub(super) fn label(ui: &Ui, rect: Rect, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        crate::native_dialog::font(ui.ctx(), size),
        color,
    );
}
pub(super) fn button(ui: &mut Ui, rect: Rect, text: &str, green: bool) -> egui::Response {
    let response = ui
        .interact(rect, ui.id().with(text), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if green {
        Color32::from_rgb(7, 193, 96)
    } else {
        Color32::from_gray(248)
    };
    ui.painter().rect_filled(rect, 0., fill);
    ui.painter().rect_stroke(
        rect,
        0.,
        egui::Stroke::new(
            1.,
            if green {
                Color32::from_rgb(6, 173, 86)
            } else {
                Color32::from_gray(204)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let color = if green {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let galley = crate::compact_menu::single_line(
        ui,
        text,
        crate::native_dialog::font(ui.ctx(), 13.),
        color,
        (rect.width() - 12.).max(0.),
    );
    let elided = galley.elided;
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, color);
    if !elided {
        response
    } else if ui.is_enabled() {
        response.on_hover_text(text)
    } else {
        response.on_disabled_hover_text(text)
    }
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft, icons: &mut Icons) -> Option<Action> {
    let origin = ui.max_rect().min;
    let size = ui.max_rect().size();
    let at = |x, y, w, h| Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
    let mut action = None;
    ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 13.));
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    ui.spacing_mut().icon_width = 12.;
    ui.spacing_mut().icon_spacing = 6.;
    ui.painter().rect_filled(ui.max_rect(), 0., Color32::WHITE);
    ui.painter()
        .rect_filled(at(0., 0., size.x, 37.), 0., Color32::from_gray(236));
    for (index, (text, x, width)) in [
        ("常规", 0., 49.),
        ("对象选择", 49., 73.),
        ("高级", 122., 49.),
        ("信息日志", 171., 73.),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = at(x, 0., width, 37.);
        let response = ui.interact(
            rect,
            ui.id().with(("backup-tab", index)),
            egui::Sense::click(),
        );
        ui.painter().rect_filled(
            rect,
            0.,
            Color32::from_gray(if draft.tab == index { 255 } else { 226 }),
        );
        ui.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            egui::Stroke::new(1., Color32::from_gray(187)),
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::BLACK,
        );
        if response.clicked() {
            draft.tab = index;
        }
    }
    let bottom = size.y - 49.;
    match draft.tab {
        0 => {
            for (caption, value, y) in [
                ("服务器:", draft.connection.name.as_str(), 37.),
                ("数据库:", draft.scope.database.as_str(), 67.),
            ] {
                label(ui, at(0., y, 80., 18.), caption, 13., Color32::BLACK);
                label(
                    ui,
                    at(90., y, size.x - 90., 18.),
                    value,
                    13.,
                    Color32::BLACK,
                );
            }
            let box_rect = at(0., 97., size.x, 175.);
            ui.painter()
                .rect_filled(box_rect, 1., Color32::from_gray(244));
            ui.painter().rect_stroke(
                box_rect,
                1.,
                egui::Stroke::new(1., Color32::from_gray(204)),
                egui::StrokeKind::Inside,
            );
            crate::native_dialog::gradient(
                ui,
                at(1., 98., size.x - 2., 23.),
                Color32::from_gray(251),
                Color32::from_gray(223),
            );
            label(
                ui,
                at(9., 99., size.x - 18., 21.),
                "备份文件选项",
                12.,
                Color32::BLACK,
            );
            label(ui, at(12., 131., 80., 18.), "注释:", 13., Color32::BLACK);
            let id =
                draft.editors[0].prepare_id(ui.ctx(), egui::Id::new(("backup-comment", &draft.id)));
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(at(12., 157., size.x - 24., 103.)),
                |ui| {
                    let scroll = egui::ScrollArea::both()
                        .id_salt("backup-comment-scroll")
                        .show(ui, |ui| {
                            ui.add_sized(
                                [size.x - 24., 103.],
                                egui::TextEdit::multiline(&mut draft.options.comment)
                                    .id(id)
                                    .desired_width(f32::INFINITY)
                                    .char_limit(262144),
                            );
                        });
                    draft.scroll_areas[0] = Some(scroll.id);
                },
            );
            draft.editors[0].trim(ui.ctx());
            label(
                ui,
                at(0., 284., size.x, 15.),
                "注意：这将只备份表和数据。不包括查询和报表。",
                11.,
                Color32::from_gray(136),
            );
        }
        1 => {
            label(
                ui,
                at(0., 37., size.x, 18.),
                "你可以选择要备份哪个数据库对象。",
                13.,
                Color32::BLACK,
            );
            label(ui, at(0., 65., size.x, 18.), "对象:", 13., Color32::BLACK);
            let rect = at(0., 93., size.x, (bottom - 141.).max(26.));
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.set_clip_rect(rect);
                if draft.loading {
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "正在加载备份对象，请稍候...",
                        crate::native_dialog::font(ui.ctx(), 13.),
                        Color32::GRAY,
                    );
                } else {
                    if draft.load_failed {
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            draft.error.as_deref().unwrap_or("加载对象失败"),
                            crate::native_dialog::font(ui.ctx(), 13.),
                            Color32::GRAY,
                        );
                    } else {
                        super::tree::tree(ui, draft, icons);
                    }
                }
            });
            for (text, x, selected) in [
                ("全选", size.x / 2. - 105., true),
                ("取消全选", size.x / 2. + 5., false),
            ] {
                if button(ui, at(x, bottom - 28., 100., 28.), text, false).clicked() {
                    for group in &mut draft.groups {
                        group.set_all(selected);
                        if !selected {
                            group.set_custom(false);
                        }
                    }
                }
            }
        }
        2 => {
            for (y, text, value) in [
                (37., "锁定全部表", &mut draft.options.lock_tables),
                (
                    70.,
                    "使用单一事务（只限 InnoDB）",
                    &mut draft.options.single_transaction,
                ),
                (103., "使用指定文件名", &mut draft.options.custom_filename),
            ] {
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(at(0., y, size.x, 18.)),
                    |ui| {
                        ui.checkbox(value, text);
                    },
                );
            }
            let id = draft.editors[1]
                .prepare_id(ui.ctx(), egui::Id::new(("backup-filename", &draft.id)));
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(at(0., 136., size.x, 26.)),
                |ui| {
                    ui.add_enabled_ui(draft.options.custom_filename, |ui| {
                        ui.add_sized(
                            [size.x, 26.],
                            egui::TextEdit::singleline(&mut draft.options.filename)
                                .id(id)
                                .char_limit(255),
                        )
                    });
                },
            );
            draft.editors[1].trim(ui.ctx());
        }
        _ => {
            let (processed, records, selected) = draft
                .control
                .read(|progress| (progress.objects, progress.records, progress.total))
                .unwrap_or_default();
            let total = if draft.started.is_some() {
                selected.to_string()
            } else {
                draft.total().to_string()
            };
            let processed = if draft.started.is_some() {
                processed.to_string()
            } else {
                "-".into()
            };
            let records = if draft.started.is_some() {
                records.to_string()
            } else {
                "-".into()
            };
            let elapsed = if draft.running {
                "进行中...".into()
            } else if draft.error.is_some() {
                "失败".into()
            } else if let Some(ms) = draft.elapsed_ms {
                format!("{:.2} 秒", ms as f64 / 1000.)
            } else {
                "-".into()
            };
            for (index, (caption, value)) in [
                ("对象:", total.as_str()),
                ("已处理对象:", processed.as_str()),
                ("已处理记录:", records.as_str()),
                ("时间:", elapsed.as_str()),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 37. + index as f32 * 23.;
                label(ui, at(0., y, 80., 18.), caption, 13., Color32::BLACK);
                label(
                    ui,
                    at(90., y, size.x - 90., 18.),
                    value,
                    13.,
                    Color32::BLACK,
                );
            }
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(at(0., 134., size.x, (bottom - 134.).max(18.))),
                |ui| {
                    super::log_view::render(ui, draft);
                },
            );
        }
    }
    ui.painter()
        .rect_filled(at(0., bottom, size.x, 49.), 0., Color32::from_gray(240));
    ui.painter().line_segment(
        [
            origin + egui::vec2(0., bottom + 0.5),
            origin + egui::vec2(size.x, bottom + 0.5),
        ],
        egui::Stroke::new(1., Color32::from_gray(221)),
    );
    let save_rect = at(size.x - 264., size.y - 38., 64., 28.);
    let save = ui.interact(
        save_rect,
        ui.id().with("backup-template-menu"),
        egui::Sense::click(),
    );
    ui.painter()
        .rect_filled(save_rect, 0., Color32::from_gray(248));
    ui.painter().rect_stroke(
        save_rect,
        0.,
        egui::Stroke::new(1., Color32::from_gray(204)),
        egui::StrokeKind::Inside,
    );
    label(
        ui,
        save_rect.shrink2(egui::vec2(10., 0.)),
        "保存",
        13.,
        Color32::BLACK,
    );
    let arrow = save_rect.right_center() - egui::vec2(12., -1.);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            arrow + egui::vec2(-4., -2.),
            arrow + egui::vec2(4., -2.),
            arrow + egui::vec2(0., 2.),
        ],
        Color32::from_gray(85),
        egui::Stroke::NONE,
    ));
    egui::Popup::menu(&save).show(|ui| {
        for (text, verb) in [
            ("保存为模板", Action::SaveTemplate),
            ("加载模板", Action::LoadTemplate),
            ("清除模板", Action::ClearTemplate),
        ] {
            if ui.button(text).clicked() {
                action = Some(verb);
                ui.close();
            }
        }
    });
    let caption = if draft.loading {
        "加载中..."
    } else if draft.load_failed {
        "加载失败"
    } else if draft.running {
        "备份中..."
    } else if draft.total() == 0 {
        "无对象"
    } else {
        "备份"
    };
    let ready = !draft.loading && !draft.running && draft.total() > 0;
    let start = ui
        .add_enabled_ui(ready, |ui| {
            button(ui, at(size.x - 190., size.y - 38., 90., 28.), caption, true)
        })
        .inner;
    if start.clicked() {
        action = Some(Action::Start);
    }
    let close = button(ui, at(size.x - 90., size.y - 38., 80., 28.), "关闭", false);
    if close.clicked() {
        action = Some(Action::Close);
    }
    if super::preview::active() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("tomato-backup-button-bounds"),
                [
                    start.rect.translate(-origin.to_vec2()),
                    close.rect.translate(-origin.to_vec2()),
                ],
            )
        });
    }
    if draft.loading || draft.running {
        ui.put(
            at(size.x - 292., size.y - 33., 18., 18.),
            egui::Spinner::new().size(18.),
        );
    }
    if ui.input(|input| input.viewport().close_requested() || input.key_pressed(egui::Key::Escape))
    {
        action = Some(Action::Close);
    }
    action
}
