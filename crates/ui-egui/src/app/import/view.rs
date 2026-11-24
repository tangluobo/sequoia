//! Original ImportConnectionDialog geometry, independent checks and modal feedback.
use super::*;
#[cfg(test)]
mod tests;
mod tree;
pub(super) enum Action {
    None,
    Cancel,
    Apply,
}
pub(super) fn render(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    icons: &mut crate::icons::Icons,
) -> Action {
    let viewport = egui::ViewportId::from_hash_of("import-connections");
    let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("import");
    let mut action = Action::None;
    if !matches!(dialog.phase, Phase::Done(_)) {
        if capture {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("导入连接")
            .with_inner_size([580., 500.])
            .with_min_inner_size([560., 430.])
            .with_active(!capture);
        let builder = crate::native_dialog::placement::prepare(
            ctx,
            viewport,
            builder,
            !dialog.positioned,
            ctx.input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect)),
            egui::vec2(580., 500.),
        );
        dialog.positioned = true;
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            crate::native_dialog::placement::correct(ui, viewport);
            if ui.input(|i| {
                i.viewport().close_requested()
                    || matches!(dialog.phase, Phase::Ready | Phase::Writing { .. })
                        && i.key_pressed(egui::Key::Escape)
            }) {
                if dialog.writing() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                action = Action::Cancel;
            }
            if capture
                && let Some(image) = ui.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot {
                            viewport_id, image, ..
                        } if *viewport_id == viewport => Some(image.clone()),
                        _ => None,
                    })
                })
            {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-image"), image));
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
            content(ui, dialog, icons, &mut action);
        });
    }
    let notice = match &dialog.phase {
        Phase::Notice { message, kind } => Some((
            message.as_str(),
            if matches!(kind, crate::native_dialog::message::Kind::Error) {
                "导入失败"
            } else {
                "提示"
            },
            *kind,
        )),
        Phase::Done(message) => Some((
            message.as_str(),
            "导入成功",
            crate::native_dialog::message::Kind::Information,
        )),
        _ => None,
    };
    if let Some((message, title, kind)) = notice {
        use crate::native_dialog::message::{self, Button, Role};
        if message::show(
            ctx,
            message::Message {
                id: egui::ViewportId::from_hash_of("connection-import-notice"),
                owner: crate::native_dialog::placement::owner_rect(ctx, viewport),
                title,
                header: None,
                content: message,
                kind,
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: !capture,
            },
            &mut dialog.notice_focus,
            icons,
            |_, _| {},
        )
        .is_some()
        {
            if matches!(dialog.phase, Phase::Done(_)) {
                action = Action::Cancel;
            } else {
                dialog.phase = Phase::Ready;
                dialog.notice_focus = true;
            }
        }
    }
    action
}
fn content(ui: &mut Ui, dialog: &mut Dialog, icons: &mut crate::icons::Icons, action: &mut Action) {
    let bounds = ui.max_rect();
    let width = bounds.width();
    let height = bounds.height();
    let rect =
        |x, y, w, h| egui::Rect::from_min_size(bounds.min + egui::vec2(x, y), egui::vec2(w, h));
    ui.painter().rect_filled(bounds, 0, Color32::from_gray(244));
    let title = egui::WidgetText::from(
        RichText::new("选择要导入的项目：").font(crate::theme::java_bold_font(ui.ctx(), 14.)),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        width - 30.,
        egui::FontSelection::Default,
    );
    ui.painter()
        .galley(bounds.min + egui::vec2(15., 15.), title, Color32::BLACK);
    let total = dialog.plan.connections.len();
    let duplicates = (0..total).filter(|i| dialog.plan.duplicate(*i)).count();
    ui.painter().text(
        bounds.min + egui::vec2(15., 50.),
        egui::Align2::LEFT_CENTER,
        format!("共 {total} 项，检测到 {duplicates} 个重复项；重复项默认不勾选，仍可手动选择。"),
        egui::FontId::proportional(12.),
        Color32::from_gray(119),
    );
    ui.add_enabled_ui(matches!(dialog.phase, Phase::Ready), |ui| {
        tree::show(
            ui,
            dialog,
            icons,
            rect(15., 65., width - 30., height - 142.),
        );
        if button(ui, rect(15., height - 69., 39., 23.), "全选", false) {
            dialog.selected = (0..total).collect();
        }
        if button(
            ui,
            rect(64., height - 69., 101., 23.),
            "仅选择未重复项",
            false,
        ) {
            dialog.selected = (0..total).filter(|i| !dialog.plan.duplicate(*i)).collect();
        }
        if button(ui, rect(175., height - 69., 64., 23.), "取消全选", false) {
            dialog.selected.clear();
        }
        if button(ui, rect(width - 95., height - 38., 80., 23.), "导入", true) {
            *action = Action::Apply;
        }
    });
    ui.add_enabled_ui(
        matches!(
            dialog.phase,
            Phase::Ready
                | Phase::Writing {
                    canceling: false,
                    ..
                }
        ),
        |ui| {
            if button(
                ui,
                rect(width - 185., height - 38., 80., 23.),
                "取消",
                false,
            ) {
                *action = Action::Cancel;
            }
        },
    );
    if let Phase::Writing { canceling, .. } = dialog.phase {
        ui.put(
            rect(15., height - 38., width - 210., 23.),
            egui::Label::new(if canceling {
                "正在取消…"
            } else {
                "正在导入…"
            })
            .halign(egui::Align::Min),
        );
    }
}
fn button(ui: &mut Ui, rect: egui::Rect, label: &str, green: bool) -> bool {
    let response = ui.interact(
        rect,
        egui::Id::new(("import-button", label)),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if green {
        Color32::from_rgb(7, 193, 96)
    } else {
        Color32::from_gray(if response.is_pointer_button_down_on() {
            224
        } else if response.hovered() {
            247
        } else {
            238
        })
    };
    ui.painter().rect_filled(rect, 3, fill);
    if !green {
        ui.painter().rect_stroke(
            rect,
            3,
            egui::Stroke::new(1., Color32::from_gray(179)),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.),
        if green {
            Color32::WHITE
        } else {
            Color32::BLACK
        },
    );
    ui.is_enabled()
        && (response.clicked()
            || response.has_focus()
                && ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                }))
}
