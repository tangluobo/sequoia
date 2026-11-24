//! Java ExportConnectionDialog: 500x450, 30-pixel rows, original assets and tri-state boxes.
use super::*;
use tomato_core::config::export::Check;
mod keyboard;
use crate::app::connection_picker::{
    self as picker,
    widgets::{button, checkbox},
};
pub(super) enum Action {
    None,
    Cancel,
    Apply,
}

pub(super) fn render(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    connections: &[Connection],
    icons: &mut crate::icons::Icons,
) -> Action {
    let viewport = egui::ViewportId::from_hash_of("export-connections");
    let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("export");
    let mut action = Action::None;
    if !matches!(dialog.phase, Phase::Done(_)) {
        if capture {
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport)
            });
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("导出连接")
            .with_inner_size([500., 450.])
            .with_min_inner_size([500., 400.])
            .with_active(!capture);
        let builder = crate::native_dialog::placement::prepare(
            ctx,
            viewport,
            builder,
            !dialog.positioned,
            ctx.input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect)),
            egui::vec2(500., 450.),
        );
        dialog.positioned = true;
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            crate::native_dialog::placement::correct(ui, viewport);
            if ui.input(|i| {
                i.viewport().close_requested()
                    || matches!(dialog.phase, Phase::Ready | Phase::Writing { .. })
                        && i.key_pressed(egui::Key::Escape)
            }) {
                action = Action::Cancel;
                return;
            }
            if capture
                && let Some(image) = ui.input(|i| {
                    i.events.iter().find_map(|e| {
                        if let egui::Event::Screenshot {
                            viewport_id, image, ..
                        } = e
                            && *viewport_id == viewport
                        {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                })
            {
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-image"), image));
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
            content(ui, dialog, connections, icons, &mut action);
        });
    }
    let notice = match &dialog.phase {
        Phase::Warning(s) => Some((
            s.as_str(),
            "提示",
            crate::native_dialog::message::Kind::Warning,
        )),
        Phase::Error(s) => Some((
            s.as_str(),
            "导出失败",
            crate::native_dialog::message::Kind::Error,
        )),
        Phase::Done(s) => Some((
            s.as_str(),
            "导出成功",
            crate::native_dialog::message::Kind::Information,
        )),
        _ => None,
    };
    if let Some((message, title, kind)) = notice {
        use crate::native_dialog::message::{self, Button, Role};
        if message::show(
            ctx,
            message::Message {
                id: egui::ViewportId::from_hash_of("connection-export-notice"),
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

pub(super) fn content(
    ui: &mut Ui,
    dialog: &mut Dialog,
    connections: &[Connection],
    icons: &mut crate::icons::Icons,
    action: &mut Action,
) {
    let bounds = ui.max_rect();
    let width = bounds.width();
    let height = bounds.height();
    let rect =
        |x, y, w, h| egui::Rect::from_min_size(bounds.min + egui::vec2(x, y), egui::vec2(w, h));
    ui.painter().rect_filled(bounds, 0, Color32::from_gray(244));
    let title = egui::WidgetText::from(
        RichText::new("选择要导出的连接：").font(crate::theme::java_bold_font(ui.ctx(), 14.)),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        width - 30.,
        egui::FontSelection::Default,
    );
    ui.painter()
        .galley(bounds.min + egui::vec2(15., 15.), title, Color32::BLACK);
    let editable = matches!(dialog.phase, Phase::Ready);
    ui.add_enabled_ui(editable, |ui| {
        let tree = dialog.selection.tree(connections);
        let mut visible = keyboard::visible(&tree, dialog);
        let tree_rect = rect(15., 44., width - 30., height - 140.);
        let tree_id = ui.id().with("export-tree-focus");
        picker::begin(ui, tree_rect, tree_id, &mut dialog.focus_tree);
        let reveal = keyboard::update(
            ui,
            tree_id,
            dialog,
            &tree,
            &mut visible,
            (tree_rect.height() / 30.).floor().max(1.) as usize,
        );
        ui.painter().rect_filled(tree_rect, 0, Color32::WHITE);
        ui.painter().rect_stroke(
            tree_rect,
            0,
            egui::Stroke::new(1., Color32::from_gray(217)),
            egui::StrokeKind::Inside,
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(tree_rect.shrink(1.)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(tree_rect));
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.spacing_mut().icon_width = 14.;
                ui.spacing_mut().interact_size.y = 14.;
                let mut area = ScrollArea::vertical()
                    .id_salt("export-tree-scroll")
                    .auto_shrink([false, false]);
                if let Some(row) = reveal {
                    area = area.vertical_scroll_offset(picker::scroll(
                        ui,
                        "export-tree-scroll",
                        row,
                        ui.available_height(),
                    ));
                }
                area.show_rows(ui, 30., visible.len(), |ui, range| {
                    for visible_index in range {
                        let index = visible[visible_index];
                        let item = &tree.rows[index];
                        let c = item.connection;
                        let (row, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 30.),
                            egui::Sense::hover(),
                        );
                        let response = ui.interact(
                            row,
                            ui.id().with(("export-row", &c.id)),
                            egui::Sense::CLICK,
                        );
                        let mut fold = response.double_clicked();
                        if response.clicked() {
                            dialog.focused = Some(c.id.clone());
                            ui.memory_mut(|m| m.request_focus(tree_id));
                        }
                        let (background, text) = picker::selected_color(ui, tree_id);
                        let offset = item.depth as f32 * 12.;
                        let focused = dialog.focused.as_deref() == Some(c.id.as_str());
                        if focused {
                            ui.painter().rect_filled(row, 0, background);
                        }
                        if item.end > index + 1 {
                            let center = egui::pos2(row.left() + 15. + offset, row.center().y);
                            let closed = dialog.collapsed.contains(&c.id);
                            let points = if closed {
                                vec![
                                    center + egui::vec2(-2., -4.),
                                    center + egui::vec2(3., 0.),
                                    center + egui::vec2(-2., 4.),
                                ]
                            } else {
                                vec![
                                    center + egui::vec2(-4., -2.),
                                    center + egui::vec2(4., -2.),
                                    center + egui::vec2(0., 3.),
                                ]
                            };
                            ui.painter().add(egui::Shape::convex_polygon(
                                points,
                                if focused {
                                    text
                                } else {
                                    Color32::from_gray(51)
                                },
                                egui::Stroke::NONE,
                            ));
                            fold |= ui
                                .interact(
                                    egui::Rect::from_center_size(center, egui::vec2(18., 30.)),
                                    ui.id().with(("export-expand", &c.id)),
                                    egui::Sense::CLICK,
                                )
                                .clicked();
                        }
                        if fold && item.end > index + 1 {
                            if !dialog.collapsed.remove(&c.id) {
                                dialog.collapsed.insert(c.id.clone());
                                if tree.rows[index + 1..item.end]
                                    .iter()
                                    .any(|row| dialog.focused.as_ref() == Some(&row.connection.id))
                                {
                                    dialog.focused = Some(c.id.clone());
                                }
                            }
                            ui.memory_mut(|m| m.request_focus(tree_id));
                            ui.ctx().request_repaint();
                        }
                        let check = egui::Rect::from_center_size(
                            egui::pos2(row.left() + 34. + offset, row.center().y),
                            egui::vec2(16., 16.),
                        );
                        if let Some(checked) = checkbox(
                            ui,
                            check,
                            ui.id().with(("export-check", &c.id)),
                            item.check == Check::Checked,
                            item.check == Check::Partial,
                            None,
                            icons,
                        ) {
                            dialog.selection.set(&tree, index, checked);
                            ui.memory_mut(|memory| memory.request_focus(tree_id));
                        }
                        let image = egui::Rect::from_center_size(
                            egui::pos2(row.left() + 52. + offset, row.center().y),
                            egui::vec2(16., 16.),
                        );
                        icons.paint(ui, &crate::icons::for_connection(c), image);
                        if ui
                            .interact(
                                image,
                                ui.id().with(("export-icon", &c.id)),
                                egui::Sense::CLICK,
                            )
                            .clicked()
                        {
                            dialog
                                .selection
                                .set(&tree, index, item.check != Check::Checked);
                            ui.memory_mut(|m| m.request_focus(tree_id));
                        }
                        let label = egui::Rect::from_min_max(
                            egui::pos2(row.left() + 63. + offset, row.top()),
                            row.max,
                        );
                        ui.painter()
                            .with_clip_rect(label.intersect(ui.clip_rect()))
                            .text(
                                label.left_center(),
                                egui::Align2::LEFT_CENTER,
                                &c.name,
                                egui::FontId::proportional(12.),
                                if focused { text } else { Color32::BLACK },
                            );
                    }
                });
            },
        );
        if button(ui, rect(15., height - 86., 46., 29.), "全选", false) {
            dialog.selection.all(connections, true);
        }
        if button(ui, rect(71., height - 86., 72., 29.), "取消全选", false) {
            dialog.selection.all(connections, false);
        }
        if let Some(checked) = checkbox(
            ui,
            rect(width - 91., height - 86., 76., 29.),
            ui.id().with("include-passwords"),
            dialog.include_passwords,
            false,
            Some("包含密码"),
            icons,
        ) {
            dialog.include_passwords = checked;
        }
        if button(ui, rect(width - 95., height - 43., 80., 29.), "导出", true) {
            *action = Action::Apply;
        }
    });
    ui.add_enabled_ui(
        matches!(dialog.phase, Phase::Ready | Phase::Writing { .. }),
        |ui| {
            if button(
                ui,
                rect(width - 185., height - 43., 80., 29.),
                "取消",
                false,
            ) {
                *action = Action::Cancel;
            }
        },
    );
    if matches!(dialog.phase, Phase::Writing { .. }) {
        ui.put(
            rect(15., height - 43., width - 210., 25.),
            egui::Label::new("正在导出…").halign(egui::Align::Min),
        );
    }
}
