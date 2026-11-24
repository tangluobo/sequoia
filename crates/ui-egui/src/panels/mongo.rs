use super::*;
use crate::tabs::mongo::{Confirmation, State};
use tomato_services::mongo::Action;
#[cfg(test)]
mod tests;

fn edit(
    ui: &mut Ui,
    text: &mut String,
    owner: &mut crate::tabs::text_state::Editor,
    name: &str,
    rows: usize,
    enabled: bool,
) {
    let id = owner.prepare(ui, name);
    let font = egui::FontId::monospace(12.);
    let mut galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(text.clone(), font.clone(), Color32::from_gray(51))
    });
    let width = ui.available_width().max(galley.size().x + 16.);
    let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _: f32| {
        if buffer.as_str() != galley.job.text {
            galley = ui.fonts_mut(|fonts| {
                fonts.layout_no_wrap(buffer.as_str().into(), font.clone(), Color32::from_gray(51))
            });
        }
        galley.clone()
    };
    let response = ui.add(
        TextEdit::multiline(text)
            .id(id)
            .code_editor()
            .font(egui::FontId::monospace(12.))
            .layouter(&mut layouter)
            .interactive(enabled)
            .desired_width(width)
            .min_size(egui::vec2(
                0.,
                if rows >= 16 {
                    ui.available_height().max(80.)
                } else {
                    0.
                },
            ))
            .desired_rows(rows),
    );
    owner.apply_focus(&response);
    owner.trim(ui.ctx());
}
fn format(text: &mut String, error: &mut Option<String>) {
    match tomato_services::mongo::parse(text).and_then(tomato_services::mongo::pretty) {
        Ok(formatted) => *text = formatted,
        Err(e) => *error = Some(format!("{e:#}")),
    }
}
pub(super) fn render(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    egui::Frame::new()
        .inner_margin(8)
        .show(ui, |ui| render_inner(ui, tab))
        .inner
}
fn render_inner(ui: &mut Ui, tab: &mut Tab) -> Option<Intent> {
    ui.spacing_mut().interact_size.y = 22.;
    ui.spacing_mut().item_spacing = egui::vec2(7., 7.);
    let Panel::Mongo { connection, view } = &mut tab.panel else {
        return None;
    };
    let mut action = if tab.busy {
        None
    } else {
        view.pending.pop_front()
    };
    egui::Panel::bottom(ui.id().with("mongo-status"))
        .exact_size(22.)
        .resizable(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.label(&view.status);
        });
    let (tabs, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.), egui::Sense::hover());
    ui.painter()
        .rect_filled(tabs, 0., Color32::from_rgb(236, 236, 234));
    let mut x = tabs.left();
    for (index, label) in ["文档", "命令", "索引", "校验规则"].iter().enumerate() {
        let font = egui::FontId::proportional(12.);
        let galley = ui.painter().layout_no_wrap(
            (*label).into(),
            font,
            Color32::from_gray(if view.page == index { 0 } else { 85 }),
        );
        let rect = egui::Rect::from_min_size(
            egui::pos2(x, tabs.top()),
            egui::vec2(galley.size().x + 24., 26.),
        );
        ui.painter().rect_filled(
            rect,
            0.,
            if view.page == index {
                Color32::from_rgb(252, 255, 255)
            } else {
                Color32::from_gray(224)
            },
        );
        ui.painter().galley(
            rect.center() - galley.size() / 2.,
            galley,
            Color32::from_gray(51),
        );
        ui.painter().vline(
            rect.right(),
            rect.y_range(),
            egui::Stroke::new(1., Color32::from_gray(192)),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(
                1.,
                Color32::from_gray(if view.page == index { 0 } else { 192 }),
            ),
        );
        if ui
            .interact(
                rect,
                ui.id().with(("mongo-page", index)),
                egui::Sense::click(),
            )
            .clicked()
        {
            view.page = index;
        }
        x = rect.right();
    }
    ui.add_enabled_ui(!tab.busy, |ui| match view.page {
        0 => documents(ui, view, &mut action, &mut tab.error),
        1 => {
            ui.label("数据库命令（JSON）");
            edit(
                ui,
                &mut view.command,
                &mut view.editors[1],
                "mongo-command",
                9,
                true,
            );
            if ui.button("执行数据库命令").clicked() {
                action = Some(Action::Command {
                    database: view.target.database.clone(),
                    json: view.command.clone(),
                });
            }
            ui.label("结果");
            ScrollArea::both()
                .id_salt("mongo-command-result")
                .show(ui, |ui| {
                    edit(
                        ui,
                        &mut view.result,
                        &mut view.editors[2],
                        "mongo-result",
                        16,
                        false,
                    )
                });
        }
        2 => {
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut view.index_keys)
                        .hint_text("索引键，例如 {\"created_at\": -1}")
                        .desired_width(180.),
                );
                ui.add(
                    TextEdit::singleline(&mut view.index_name)
                        .hint_text("索引名（可选）")
                        .desired_width(120.),
                );
                ui.checkbox(&mut view.unique, "唯一索引");
                if ui.button("创建索引").clicked() {
                    action = Some(Action::CreateIndex {
                        target: view.target.clone(),
                        keys: view.index_keys.clone(),
                        name: view.index_name.clone(),
                        unique: view.unique,
                    });
                }
                if ui.button("删除选中索引").clicked() {
                    if let Some(json) = view.selected_index.and_then(|i| view.indexes.get(i)) {
                        match tomato_services::mongo::parse(json)
                            .and_then(|d| Ok(d.get_str("name")?.to_owned()))
                        {
                            Ok(name) => view.confirm = Some(Confirmation::Index(name)),
                            Err(e) => tab.error = Some(format!("{e:#}")),
                        }
                    } else {
                        tab.error = Some("请先选择要删除的索引".into());
                    }
                }
                if ui.button("刷新").clicked() {
                    action = Some(Action::Indexes {
                        target: view.target.clone(),
                    });
                }
            });
            if let Some(index) =
                json_list(ui, &view.indexes, &view.index_labels, view.selected_index)
            {
                view.selected_index = Some(index);
            }
        }
        _ => {
            ui.label(
                "集合设计使用 MongoDB validator / $jsonSchema；保存后以 strict + error 模式生效。",
            );
            ui.horizontal(|ui| {
                if ui.button("保存校验规则").clicked() {
                    action = Some(Action::UpdateValidator {
                        target: view.target.clone(),
                        json: view.validator.clone(),
                    });
                }
                if ui.button("重新加载").clicked() {
                    action = Some(Action::Validator {
                        target: view.target.clone(),
                    });
                }
                if ui.button("格式化 JSON").clicked() {
                    format(&mut view.validator, &mut tab.error);
                }
            });
            ScrollArea::both()
                .id_salt("mongo-validator")
                .show(ui, |ui| {
                    edit(
                        ui,
                        &mut view.validator,
                        &mut view.editors[3],
                        "mongo-validator-text",
                        32,
                        true,
                    )
                });
        }
    });
    if let Some(confirm) = &view.confirm {
        let mut accept = false;
        let mut cancel = false;
        egui::Window::new(match confirm {
            Confirmation::Document => "删除 MongoDB 文档",
            Confirmation::Index(_) => "删除 MongoDB 索引",
        })
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.label(match confirm {
                Confirmation::Document => "确定删除选中的文档吗？此操作不可撤销。".into(),
                Confirmation::Index(name) => format!("确定删除索引 “{name}” 吗？"),
            });
            ui.horizontal(|ui| {
                accept = ui
                    .add_enabled(!tab.busy, egui::Button::new("确定"))
                    .clicked();
                cancel = ui.button("取消").clicked();
            });
        });
        if accept {
            action = match confirm {
                Confirmation::Document => view.original().map(|json| Action::Delete {
                    target: view.target.clone(),
                    json: json.into(),
                }),
                Confirmation::Index(name) => Some(Action::DropIndex {
                    target: view.target.clone(),
                    name: name.clone(),
                }),
            };
        }
        if cancel {
            view.confirm = None;
        }
    }
    action.map(|action| {
        if matches!(action, Action::Find { .. }) {
            view.status = "正在查询文档...".into();
        }
        Intent::command(Command::Mongo {
            connection: (**connection).clone(),
            action,
        })
    })
}
fn documents(
    ui: &mut Ui,
    view: &mut State,
    action: &mut Option<Action>,
    error: &mut Option<String>,
) {
    ui.horizontal(|ui| {
        ui.label("筛选：");
        let width = (ui.available_width() - 580.).max(80.);
        let filter = ui.add(
            TextEdit::singleline(&mut view.filter)
                .desired_width(width)
                .hint_text("MongoDB JSON 查询条件"),
        );
        ui.label("跳过");
        spinner(ui, &mut view.skip, 0, i32::MAX as u64, "skip");
        ui.label("条数");
        let mut limit = u64::from(view.limit);
        spinner(ui, &mut limit, 1, 1000, "limit");
        view.limit = limit as u32;
        if ui.button("查询").clicked()
            || (filter.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            *action = Some(view.find());
        }
        if ui.button("上一页").clicked() {
            view.skip = view.skip.saturating_sub(u64::from(view.limit));
            *action = Some(view.find());
        }
        if ui.button("下一页").clicked() {
            view.skip = view
                .skip
                .saturating_add(u64::from(view.limit))
                .min(i32::MAX as u64);
            *action = Some(view.find());
        }
    });
    ui.add_space(7.);
    let width = ui.available_width();
    egui::Panel::left(ui.id().with("mongo-documents"))
        .default_size(width * 0.42)
        .min_size(120.)
        .max_size((width * 0.7).max(120.))
        .show(ui, |ui| {
            if let Some(index) =
                json_list(ui, &view.documents, &view.document_labels, view.selected)
            {
                view.select(index);
            }
        });
    egui::CentralPanel::default().show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("新建文档").clicked() {
                view.selected = None;
                view.text = "{\n  \n}".into();
                view.editors[0].reset();
                view.editors[0].focus();
            }
            if ui.button("保存").clicked() {
                *action = Some(if let Some(original) = view.original() {
                    Action::Replace {
                        target: view.target.clone(),
                        original: original.into(),
                        json: view.text.clone(),
                    }
                } else {
                    Action::Insert {
                        target: view.target.clone(),
                        json: view.text.clone(),
                    }
                });
            }
            if ui.button("删除").clicked() {
                if view.original().is_some() {
                    view.confirm = Some(Confirmation::Document);
                } else {
                    *error = Some("请先选择要删除的文档".into());
                }
            }
            if ui.button("格式化 JSON").clicked() {
                format(&mut view.text, error);
            }
        });
        ScrollArea::both()
            .id_salt("mongo-document-editor")
            .show(ui, |ui| {
                edit(
                    ui,
                    &mut view.text,
                    &mut view.editors[0],
                    "mongo-document",
                    32,
                    true,
                )
            });
    });
}
fn json_list(
    ui: &mut Ui,
    texts: &[String],
    labels: &[String],
    selected: Option<usize>,
) -> Option<usize> {
    let width = labels
        .iter()
        .map(|s| {
            s.chars()
                .map(|c| if c.is_ascii() { 1 } else { 2 })
                .sum::<usize>() as f32
                * 7.2
                + 8.
        })
        .fold(ui.available_width(), f32::max);
    let rows = texts
        .len()
        .max((ui.available_height() / 20.).ceil() as usize);
    let mut clicked = None;
    ScrollArea::both()
        .auto_shrink([false, false])
        .show_rows(ui, 20., rows, |ui, rows| {
            ui.spacing_mut().item_spacing.y = 0.;
            for index in rows {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 20.), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.,
                    if selected == Some(index) {
                        Color32::from_gray(211)
                    } else if index % 2 == 1 {
                        Color32::from_gray(248)
                    } else {
                        Color32::WHITE
                    },
                );
                if let Some(label) = labels.get(index) {
                    ui.painter().text(
                        rect.min + egui::vec2(4., 3.),
                        egui::Align2::LEFT_TOP,
                        label,
                        egui::FontId::monospace(12.),
                        Color32::BLACK,
                    );
                    if response.clicked() {
                        clicked = Some(index);
                    }
                    response.on_hover_text(&texts[index]);
                }
            }
        });
    clicked
}
fn spinner(ui: &mut Ui, value: &mut u64, min: u64, max: u64, name: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(160., 24.), egui::Sense::click());
    let before = *value;
    let top = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 18., rect.top()),
        egui::pos2(rect.right(), rect.center().y),
    );
    let bottom = egui::Rect::from_min_max(top.left_bottom(), rect.right_bottom());
    let up = ui.interact(top, ui.id().with((name, "up")), egui::Sense::click());
    let down = ui.interact(bottom, ui.id().with((name, "down")), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let key_up = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
    let key_down = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
    if up.clicked() || key_up {
        *value = value.saturating_add(50).min(max);
    }
    if down.clicked() || key_down {
        *value = value.saturating_sub(50).max(min);
    }
    if *value != before {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter();
    painter.rect(
        rect,
        2.,
        Color32::WHITE,
        egui::Stroke::new(1., Color32::from_gray(190)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_center() + egui::vec2(7., 0.),
        egui::Align2::LEFT_CENTER,
        value.to_string(),
        egui::FontId::proportional(12.),
        Color32::from_gray(51),
    );
    for (rect, up, hover) in [(top, true, up.hovered()), (bottom, false, down.hovered())] {
        painter.rect_filled(rect, 0., Color32::from_gray(if hover { 225 } else { 240 }));
        let c = rect.center();
        let direction = if up { -1. } else { 1. };
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(-4., -direction * 2.),
                c + egui::vec2(4., -direction * 2.),
                c + egui::vec2(0., direction * 2.),
            ],
            Color32::from_gray(100),
            egui::Stroke::NONE,
        ));
    }
}
