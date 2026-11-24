use super::*;
use crate::app::connection_picker as picker;

pub(super) fn show(
    ui: &mut Ui,
    dialog: &mut Dialog,
    icons: &mut crate::icons::Icons,
    rect: egui::Rect,
) {
    let id = ui.id().with("import-tree-focus");
    picker::begin(ui, rect, id, &mut dialog.focus_tree);
    let topology = |index: usize| {
        let row = &dialog.plan.rows()[index];
        (row.depth, row.end)
    };
    let mut visible = picker::visible(dialog.plan.rows().len(), topology, |index| {
        dialog.collapsed.contains(&dialog.plan.rows()[index].index)
    });
    let selected = dialog
        .focused
        .and_then(|item| dialog.plan.rows().iter().position(|r| r.index == item));
    let mut reveal = None;
    if let Some(key) = picker::key(ui, id) {
        let closed =
            selected.is_some_and(|row| dialog.collapsed.contains(&dialog.plan.rows()[row].index));
        if let Some(nav) = picker::navigate(
            key,
            &visible,
            selected,
            topology,
            closed,
            (rect.height() / 30.).floor().max(1.) as usize,
        ) {
            let item = dialog.plan.rows()[nav.row].index;
            if let Some(closed) = nav.collapsed {
                if closed {
                    dialog.collapsed.insert(item);
                } else {
                    dialog.collapsed.remove(&item);
                }
            }
            dialog.focused = Some(item);
            visible = picker::visible(dialog.plan.rows().len(), topology, |index| {
                dialog.collapsed.contains(&dialog.plan.rows()[index].index)
            });
            reveal = visible.iter().position(|row| *row == nav.row);
            ui.ctx().request_repaint();
        }
    }
    ui.painter().rect_filled(rect, 0, Color32::WHITE);
    ui.painter().rect_stroke(
        rect,
        0,
        egui::Stroke::new(1., Color32::from_gray(217)),
        egui::StrokeKind::Inside,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(1.)), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(rect));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let mut area = ScrollArea::vertical()
            .id_salt("import-tree-scroll")
            .auto_shrink([false, false]);
        if let Some(row) = reveal {
            area = area.vertical_scroll_offset(picker::scroll(
                ui,
                "import-tree-scroll",
                row,
                ui.available_height(),
            ));
        }
        area.show_rows(ui, 30., visible.len(), |ui, range| {
            for index in range.map(|index| visible[index]) {
                let item = dialog.plan.rows()[index];
                let connection = &dialog.plan.connections[item.index];
                let (row, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.),
                    egui::Sense::hover(),
                );
                let response = ui.interact(
                    row,
                    ui.id().with(("import-row", item.index)),
                    egui::Sense::CLICK,
                );
                let focused = dialog.focused == Some(item.index);
                let (background, text) = picker::selected_color(ui, id);
                if focused {
                    ui.painter().rect_filled(row, 0, background);
                }
                let offset = item.depth as f32 * 12.;
                let mut fold = response.double_clicked();
                if response.clicked() {
                    dialog.focused = Some(item.index);
                    ui.memory_mut(|m| m.request_focus(id));
                }
                if item.end > index + 1 {
                    let center = egui::pos2(row.left() + 15. + offset, row.center().y);
                    let closed = dialog.collapsed.contains(&item.index);
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
                            ui.id().with(("import-expand", item.index)),
                            egui::Sense::CLICK,
                        )
                        .clicked();
                }
                let check = egui::Rect::from_center_size(
                    egui::pos2(row.left() + 34. + offset, row.center().y),
                    egui::vec2(16., 16.),
                );
                let toggled = picker::widgets::checkbox(
                    ui,
                    check,
                    ui.id().with(("import-check", item.index)),
                    dialog.selected.contains(&item.index),
                    false,
                    None,
                    icons,
                )
                .is_some();
                let image = egui::Rect::from_center_size(
                    egui::pos2(row.left() + 52. + offset, row.center().y),
                    egui::vec2(16., 16.),
                );
                icons.paint(ui, &crate::icons::for_connection(connection), image);
                let image_clicked = ui
                    .interact(
                        image,
                        ui.id().with(("import-icon", item.index)),
                        egui::Sense::CLICK,
                    )
                    .clicked();
                if toggled || image_clicked {
                    if !dialog.selected.remove(&item.index) {
                        dialog.selected.insert(item.index);
                    }
                    ui.memory_mut(|m| m.request_focus(id));
                }
                let name = if connection.name.trim().is_empty() {
                    "未命名连接"
                } else {
                    &connection.name
                };
                let label = if dialog.plan.duplicate(item.index) {
                    format!("{name}  （配置已存在）")
                } else {
                    name.to_owned()
                };
                let label_rect = egui::Rect::from_min_max(
                    egui::pos2(row.left() + 63. + offset, row.top()),
                    row.max,
                );
                ui.painter()
                    .with_clip_rect(label_rect.intersect(ui.clip_rect()))
                    .text(
                        label_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(12.),
                        if focused { text } else { Color32::BLACK },
                    );
                if fold && item.end > index + 1 {
                    if !dialog.collapsed.remove(&item.index) {
                        dialog.collapsed.insert(item.index);
                        if dialog.plan.rows()[index + 1..item.end]
                            .iter()
                            .any(|r| Some(r.index) == dialog.focused)
                        {
                            dialog.focused = Some(item.index);
                        }
                    }
                    ui.memory_mut(|m| m.request_focus(id));
                    ui.ctx().request_repaint();
                }
            }
        });
    });
}
