use super::*;
const WIDTHS: [f32; 7] = [170., 75., 100., 80., 100., 190., 130.];
pub(super) fn render(
    ui: &mut Ui,
    bounds: egui::Rect,
    workspace: &mut Workspace,
    busy: bool,
) -> Option<Form> {
    let pane = workspace.panes.entry(Section::Storage).or_default();
    let mut action = None;
    if bounds.width() < 2. || bounds.height() < 26. {
        return None;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bounds.shrink(1.)));
    child.set_clip_rect(bounds.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = egui::Vec2::ZERO;
    child.painter().rect_filled(bounds, 0, Color32::WHITE);
    child.painter().rect_stroke(
        bounds,
        0,
        egui::Stroke::new(1., Color32::from_gray(190)),
        egui::StrokeKind::Inside,
    );
    let Some(table) = &pane.table else {
        child.label("正在读取存储信息...");
        return None;
    };
    let focus = child.make_persistent_id("storage-devices-focus");
    let background = child.interact(bounds, focus, egui::Sense::click());
    if !busy && background.clicked() {
        background.request_focus();
    }
    if workspace.storage.order.len() != table.rows.len() {
        workspace.storage.sort_rows(&table.rows);
    }
    let mut scroll = None;
    if !busy
        && let Some(key) = input::take(
            &child,
            focus,
            &[
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::Home,
                egui::Key::End,
                egui::Key::Enter,
            ],
        )
    {
        let selected = pane
            .selected
            .first()
            .and_then(|i| workspace.storage.order.iter().position(|j| j == i));
        if let Some(index) = input::offset(selected, workspace.storage.order.len(), key) {
            pane.selected.clear();
            pane.selected.insert(workspace.storage.order[index]);
            scroll = Some(index);
        } else if key == egui::Key::Enter
            && let Some(row) = pane.selected.first().and_then(|i| table.rows.get(*i))
        {
            action = Some(if row.get(6).is_some_and(|s| s == "未分区") {
                Form::InitializePv
            } else {
                Form::Mount
            });
        }
    }
    egui::ScrollArea::horizontal()
        .id_salt("storage-device-x")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            let mut widths = workspace.storage.widths.unwrap_or(WIDTHS);
            let policy = if workspace.storage.widths.is_some() {
                super::super::columns::Policy::Last
            } else {
                super::super::columns::Policy::Even
            };
            super::super::columns::fit(&mut widths, &[10.; 7], ui.available_width(), policy);
            super::super::columns::snap(&mut widths, ui.ctx().pixels_per_point());
            let width = widths.iter().sum();
            let (header, _) = ui.allocate_exact_size(egui::vec2(width, 24.), egui::Sense::hover());
            resize(ui, header, &mut widths, &mut workspace.storage.widths);
            ui.painter().rect_filled(header, 0, Color32::from_gray(245));
            cells(ui, header, &table.columns, &widths, true);
            let mut x = header.left();
            for (column, width) in widths.iter().enumerate() {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x, header.top()),
                    egui::vec2(*width, header.height()),
                );
                if ui
                    .interact(
                        rect,
                        ui.make_persistent_id(("storage-sort", column)),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    workspace.storage.sort = match workspace.storage.sort {
                        Some((old, false)) if old == column => Some((column, true)),
                        Some((old, true)) if old == column => None,
                        _ => Some((column, false)),
                    };
                    workspace.storage.sort_rows(&table.rows);
                }
                if let Some((selected, descending)) = workspace.storage.sort
                    && selected == column
                {
                    ui.painter().text(
                        rect.right_center() - egui::vec2(10., 0.),
                        egui::Align2::CENTER_CENTER,
                        if descending { "▾" } else { "▴" },
                        crate::theme::java_font(11.),
                        Color32::from_gray(70),
                    );
                }
                x += width;
            }
            ui.painter().hline(
                header.x_range(),
                header.bottom() - 0.5,
                egui::Stroke::new(1., Color32::from_gray(190)),
            );
            let body = ui.available_rect_before_wrap();
            let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
            for index in 0..(body.height() / 24.).ceil().max(0.) as usize {
                let row = egui::Rect::from_min_size(
                    body.min + egui::vec2(0., index as f32 * 24.),
                    egui::vec2(width, 24.),
                );
                painter.rect_filled(
                    row,
                    0,
                    Color32::from_gray(if index % 2 == 0 { 255 } else { 248 }),
                );
                painter.hline(
                    row.x_range(),
                    row.bottom() - 0.5,
                    egui::Stroke::new(1., Color32::from_gray(235)),
                );
            }
            let mut x = body.left();
            for value in &widths {
                x += value;
                painter.vline(
                    x - 0.5,
                    body.y_range(),
                    egui::Stroke::new(1., Color32::from_gray(235)),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt("storage-device-y")
                .auto_shrink([false, false])
                .show_rows(ui, 24., table.rows.len(), |ui, range| {
                    if let Some(index) = scroll {
                        let top = ui.cursor().top() - range.start as f32 * 24.;
                        ui.scroll_to_rect(
                            egui::Rect::from_min_size(
                                egui::pos2(ui.cursor().left(), top + index as f32 * 24.),
                                egui::vec2(width, 24.),
                            ),
                            Some(egui::Align::Center),
                        );
                    }
                    for display in range {
                        let index = workspace.storage.order[display];
                        let row = &table.rows[index];
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(width, 24.), egui::Sense::click());
                        ui.painter().rect_filled(
                            rect,
                            0,
                            Color32::from_gray(if display % 2 == 0 { 255 } else { 248 }),
                        );
                        if pane.selected.contains(&index) || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0,
                                if pane.selected.contains(&index) {
                                    crate::theme::TREE_SELECTED
                                } else {
                                    crate::theme::TREE_HOVER
                                },
                            );
                        }
                        cells(ui, rect, row, &widths, false);
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom() - 0.5,
                            egui::Stroke::new(1., Color32::from_gray(235)),
                        );
                        let mut x = rect.left();
                        for value in &widths {
                            x += value;
                            ui.painter().vline(
                                x - 0.5,
                                rect.y_range(),
                                egui::Stroke::new(1., Color32::from_gray(235)),
                            );
                        }
                        if !busy && (response.clicked() || response.secondary_clicked()) {
                            pane.selected.clear();
                            pane.selected.insert(index);
                            ui.memory_mut(|m| m.request_focus(focus));
                        }
                        if !busy && response.double_clicked() {
                            action = Some(if row.get(6).is_some_and(|s| s == "未分区") {
                                Form::InitializePv
                            } else {
                                Form::Mount
                            });
                        }
                    }
                });
        });
    ui.painter().rect_stroke(
        bounds,
        0,
        egui::Stroke::new(1., Color32::from_gray(190)),
        egui::StrokeKind::Inside,
    );
    action
}
fn resize(ui: &mut Ui, header: egui::Rect, widths: &mut [f32; 7], saved: &mut Option<[f32; 7]>) {
    let mut x = header.left();
    for column in 0..6 {
        x += widths[column];
        let handle = egui::Rect::from_center_size(
            egui::pos2(x, header.center().y),
            egui::vec2(7., header.height()),
        );
        let response = ui
            .interact(
                handle,
                ui.make_persistent_id(("storage-width", column)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let start_id = response.id.with("start");
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(start_id, [widths[column], widths[6]]));
        }
        if response.dragged() {
            let start = ui
                .data(|data| data.get_temp::<[f32; 2]>(start_id))
                .unwrap_or([widths[column], widths[6]]);
            let delta = response
                .total_drag_delta()
                .unwrap_or_default()
                .x
                .clamp(10. - start[0], start[1] - 10.);
            widths[column] = start[0] + delta;
            widths[6] = start[1] - delta;
            *saved = Some(*widths);
        }
        if response.drag_stopped() {
            ui.data_mut(|data| data.remove::<[f32; 2]>(start_id));
        }
    }
}
fn cells(ui: &Ui, rect: egui::Rect, values: &[String], widths: &[f32], header: bool) {
    let mut x = rect.left();
    for (value, width) in values.iter().zip(widths) {
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*width, rect.height()));
        ui.painter()
            .with_clip_rect(cell.shrink2(egui::vec2(3., 0.)).intersect(ui.clip_rect()))
            .text(
                if header {
                    cell.center()
                } else {
                    cell.left_center() + egui::vec2(5., 0.)
                },
                if header {
                    egui::Align2::CENTER_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                value,
                crate::theme::java_font(12.),
                Color32::from_gray(51),
            );
        x += width;
        if header {
            ui.painter().vline(
                x - 0.5,
                rect.y_range(),
                egui::Stroke::new(1., Color32::from_gray(210)),
            );
        }
    }
}
