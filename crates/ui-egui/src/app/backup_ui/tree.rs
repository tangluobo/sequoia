use super::view::label;
use super::{draft::Draft, tree_input};
use crate::icons::Icons;
use egui::{Color32, Rect, Ui};
pub(super) fn tree(ui: &mut Ui, draft: &mut Draft, icons: &mut Icons) {
    let area = ui.max_rect();
    let tree_id = ui.id().with("backup-tree-focus");
    let _ = ui.interact(area, tree_id, egui::Sense::click());
    tree_input::keyboard(ui, draft, tree_id, area.height());
    let count = draft.groups.iter().map(tree_input::rows).sum::<usize>();
    let total = count as f32 * 26.;
    let overflow = total > area.height();
    let content = Rect::from_min_max(
        area.min,
        area.max - egui::vec2(if overflow { 13. } else { 0. }, 0.),
    );
    let selected = &mut draft.tree_selected;
    let output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("backup-object-tree")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .auto_shrink([false, false])
                .vertical_scroll_offset(draft.tree_scroll)
                .show_rows(ui, 26., count, |ui, range| {
                    for mut index in range {
                        for group in &mut draft.groups {
                            let rows = tree_input::rows(group);
                            if index >= rows {
                                index -= rows;
                                continue;
                            }
                            let (_, rect) =
                                ui.allocate_space(egui::vec2(ui.available_width(), 26.));
                            let row_id = ui.id().with((group.kind.archive_name(), index, "row"));
                            let response = ui.interact(rect, row_id, egui::Sense::CLICK);
                            let key = (group.kind, index);
                            if response.clicked() {
                                *selected = Some(key);
                                ui.memory_mut(|memory| memory.request_focus(tree_id));
                            }
                            if *selected == Some(key) {
                                ui.painter().rect_filled(
                                    rect,
                                    0.,
                                    Color32::from_rgb(201, 235, 198),
                                );
                            } else if ui.rect_contains_pointer(rect) {
                                ui.painter().rect_filled(
                                    rect,
                                    0.,
                                    Color32::from_rgb(221, 242, 219),
                                );
                            }
                            let (indent, branch) = if index == 0 {
                                (0., true)
                            } else if index <= 2 {
                                (12., index == 2 && !group.items.is_empty())
                            } else {
                                (24., false)
                            };
                            let origin = rect.min;
                            if branch {
                                let hit = Rect::from_min_size(
                                    origin + egui::vec2(indent + 2., 0.),
                                    egui::vec2(16., 26.),
                                );
                                let expanded = if index == 0 {
                                    group.expanded
                                } else {
                                    group.custom_expanded
                                };
                                let response_arrow =
                                    ui.interact(hit, row_id.with("arrow"), egui::Sense::CLICK);
                                let points = if expanded {
                                    vec![
                                        origin + egui::vec2(indent + 9., 5.),
                                        origin + egui::vec2(indent + 17., 5.),
                                        origin + egui::vec2(indent + 13., 10.),
                                    ]
                                } else {
                                    vec![
                                        origin + egui::vec2(indent + 11., 3.),
                                        origin + egui::vec2(indent + 11., 11.),
                                        origin + egui::vec2(indent + 16., 7.),
                                    ]
                                };
                                ui.painter().add(egui::Shape::convex_polygon(
                                    points,
                                    Color32::from_gray(55),
                                    egui::Stroke::NONE,
                                ));
                                if response_arrow.clicked() || response.double_clicked() {
                                    tree_input::toggle(group, index, selected);
                                }
                            }
                            let mut text_x = indent + 22.;
                            if index > 0 {
                                let checked = if index == 1 {
                                    group.all
                                } else if index == 2 {
                                    group.custom
                                } else {
                                    group.items[index - 3].selected
                                };
                                let check = Rect::from_min_size(
                                    origin + egui::vec2(indent + 22., 6.),
                                    egui::vec2(12., 15.),
                                );
                                if let Some(value) = super::checkbox::checkbox(
                                    ui,
                                    check,
                                    row_id.with("check"),
                                    checked,
                                    index == 2 && group.indeterminate(),
                                    icons,
                                ) {
                                    *selected = Some(key);
                                    if index == 1 {
                                        group.set_all(value);
                                    } else if index == 2 {
                                        group.set_custom(value);
                                    } else {
                                        group.set_item(index - 3, value);
                                    }
                                }
                                text_x += 14.;
                            }
                            let texture = icons.get(ui.ctx(), group.icon());
                            let icon_width = if index == 0 {
                                if texture.is_some() { 24. } else { 2. }
                            } else {
                                22.
                            };
                            if let Some(texture) = texture {
                                ui.painter().image(
                                    texture,
                                    Rect::from_min_size(
                                        origin + egui::vec2(text_x, 3.),
                                        egui::vec2(20., 20.),
                                    ),
                                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                    Color32::WHITE,
                                );
                            }
                            let text = if index == 0 {
                                group.kind.label().into()
                            } else if index == 1 {
                                format!("运行期间的全部{}(*)", group.kind.label())
                            } else if index == 2 {
                                format!("自定义 ({}/{})", group.selected_count(), group.items.len())
                            } else {
                                let object = &group.items[index - 3].object;
                                if draft.connection.kind() == "POSTGRESQL"
                                    && draft.namespace.is_none()
                                {
                                    format!("{}.{}", object.table.schema, object.table.name)
                                } else {
                                    object.table.name.clone()
                                }
                            };
                            label(
                                ui,
                                Rect::from_min_max(
                                    origin + egui::vec2(text_x + icon_width, 0.),
                                    rect.max,
                                ),
                                &text,
                                12.,
                                Color32::BLACK,
                            );
                            break;
                        }
                    }
                })
        })
        .inner;
    draft.tree_scroll = output.state.offset.y;
    draft.scroll_areas[1] = Some(output.id);
    if overflow {
        let track = Rect::from_min_max(egui::pos2(content.right(), area.top()), area.max);
        crate::native_scrollbar::show(
            ui,
            track,
            true,
            &mut draft.tree_scroll,
            crate::native_scrollbar::Metrics {
                extent: area.height(),
                total,
                unit: 26.,
            },
            ui.id().with("backup-tree-scrollbar"),
            &mut draft.scrollbar,
        );
    } else {
        draft.scrollbar = Default::default();
    }
    ui.memory_mut(|memory| {
        if memory.has_focus(tree_id) {
            memory.set_focus_lock_filter(
                tree_id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        }
    });
}
