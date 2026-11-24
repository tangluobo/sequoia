use super::*;
use crate::tabs::ssh::Pane;
pub(super) fn render(
    ui: &mut Ui,
    pane: &mut Pane,
    section: Section,
    busy: bool,
    bounded: bool,
) -> (Option<Vec<String>>, Option<String>) {
    let Some(data) = &pane.table else {
        ui.label("正在读取…");
        return (None, None);
    };
    let columns = &data.columns;
    let rows = &data.rows;
    let mut widths: Vec<f32> = columns
        .iter()
        .map(|c| match c.as_str() {
            "容器ID" => 90.,
            "镜像" => 130.,
            "名称" => 110.,
            "状态" => 100.,
            "协议" => 60.,
            "本地地址" => 140.,
            "端口" => 80.,
            "PID" => 70.,
            "进程" => 180.,
            "端口映射" => 140.,
            "设备" => 170.,
            "挂载点" => 190.,
            "创建时间" => 120.,
            "描述" => 230.,
            _ => 90.,
        })
        .collect();
    let visible: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            section != Section::Ports
                || r.first().is_some_and(|s| {
                    (pane.tcp && s.starts_with("tcp")) || (pane.udp && s.starts_with("udp"))
                })
        })
        .map(|(i, _)| i)
        .collect();
    if section == Section::Ports && pane.status.starts_with("共 ") {
        use std::fmt::Write as _;
        pane.status.clear();
        let _ = write!(
            &mut pane.status,
            "共 {} 个监听端口（总计 {}）",
            visible.len(),
            rows.len()
        );
    }
    let scroll_width = if bounded && visible.len() as f32 * 24. > ui.available_height() - 24. {
        ui.spacing().scroll.allocated_width()
    } else {
        0.
    };
    let available = ui.available_width() - 16. - scroll_width;
    let minimums: Vec<f32> = columns
        .iter()
        .map(|name| match (section, name.as_str()) {
            (Section::Ports, "本地地址" | "进程") => 60.,
            (Section::Ports, "端口") => 50.,
            (Section::Docker, "容器ID") => 70.,
            (Section::Docker, "镜像" | "创建时间") => 80.,
            (Section::Docker, _) => 60.,
            _ => 40.,
        })
        .collect();
    if let Some(saved) = &pane.manager_widths
        && saved.len() == widths.len()
    {
        widths.copy_from_slice(saved);
    }
    super::super::columns::fit(
        &mut widths,
        &minimums,
        available,
        super::super::columns::Policy::Even,
    );
    super::super::columns::snap(&mut widths, ui.ctx().pixels_per_point());
    let mut chosen = None;
    let mut action = None;
    ui.spacing_mut().item_spacing.y = 0.;
    egui::ScrollArea::horizontal()
        .id_salt("manager-columns")
        .auto_shrink([false, !bounded])
        .show(ui, |ui| {
            let width = widths.iter().sum::<f32>() + 15.;
            let (header, _) = ui.allocate_exact_size(egui::vec2(width, 24.), egui::Sense::hover());
            resize(ui, header, &mut widths, &minimums, &mut pane.manager_widths);
            ui.painter().rect_filled(header, 0, Color32::from_gray(245));
            paint_columns(
                ui,
                header.top(),
                header.left() + 15.,
                columns,
                &widths,
                true,
            );
            let mut x = header.left() + 15.;
            for value in std::iter::once(&0.).chain(&widths) {
                x += value;
                ui.painter().vline(
                    x - 0.5,
                    header.y_range(),
                    egui::Stroke::new(1., Color32::from_gray(224)),
                );
            }
            ui.painter().hline(
                header.x_range(),
                header.bottom() - 0.5,
                egui::Stroke::new(1., Color32::from_gray(220)),
            );
            let viewport_height = ui.available_height();
            let mut body = |ui: &mut Ui| {
                let row_h = 24.;
                let height =
                    (visible.len() as f32 * row_h).max(if bounded { viewport_height } else { 56. });
                let (body, _) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                let bounds = egui::Rect::from_min_max(body.min - egui::vec2(0., 24.), body.max);
                selection::update(
                    ui,
                    bounds,
                    &visible,
                    &mut pane.selected,
                    &mut pane.anchor,
                    section,
                );
                let clip = body.intersect(ui.clip_rect());
                let painter = ui.painter().with_clip_rect(clip);
                let first = ((clip.top() - body.top()) / row_h).floor().max(0.) as usize;
                let last = ((clip.bottom() - body.top()) / row_h).ceil().max(0.) as usize;
                for display in first..last {
                    let rect = egui::Rect::from_min_size(
                        body.min + egui::vec2(0., display as f32 * row_h),
                        egui::vec2(width, row_h),
                    );
                    painter.rect_filled(
                        rect,
                        0,
                        Color32::from_gray(if display >= visible.len() || display % 2 == 0 {
                            255
                        } else {
                            248
                        }),
                    );
                }
                let start = first;
                let end = last.min(visible.len());
                for (display, &index) in visible.iter().enumerate().take(end).skip(start) {
                    let row = &rows[index];
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(bounds.left(), bounds.top() + (display + 1) as f32 * row_h),
                        egui::vec2(bounds.width(), row_h),
                    );
                    let response =
                        ui.interact(rect, ui.make_persistent_id(index), egui::Sense::click());
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
                    if pane.selected.contains(&index) {
                        let p = rect.left_center() + egui::vec2(5., 0.);
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                p + egui::vec2(0., -5.),
                                p + egui::vec2(5., 0.),
                                p + egui::vec2(0., 5.),
                            ],
                            Color32::BLACK,
                            egui::Stroke::NONE,
                        ));
                    }
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        egui::Stroke::new(1., Color32::from_gray(224)),
                    );
                    paint_columns(ui, rect.top(), rect.left() + 15., row, &widths, false);
                    if section == Section::Ports
                        && let Some(port) = row.get(2).and_then(|p| p.parse::<u16>().ok())
                        && let Some(kind) = management::whitelist(&data.detail, &row[0], port)
                    {
                        ui.painter().circle_filled(
                            egui::pos2(
                                rect.left() + 15. + widths[0] + widths[1] + widths[2] - 10.,
                                rect.center().y,
                            ),
                            3.,
                            if kind == management::Whitelist::Direct {
                                Color32::from_rgb(76, 175, 80)
                            } else {
                                Color32::from_rgb(33, 150, 243)
                            },
                        );
                    }
                    if section == Section::Docker {
                        let status = row
                            .get(3)
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        ui.painter().circle_filled(
                            egui::pos2(
                                rect.left() + 15. + widths[0] + widths[1] + widths[2] + widths[3]
                                    - 10.,
                                rect.center().y,
                            ),
                            3.,
                            if status.starts_with("up") {
                                Color32::from_rgb(76, 175, 80)
                            } else if status.starts_with("exited") {
                                Color32::from_gray(153)
                            } else {
                                Color32::from_rgb(255, 167, 38)
                            },
                        );
                    }
                    if response.double_clicked() {
                        chosen = Some(row.clone());
                    }
                    response.context_menu(|ui| {
                        if let Some(label) = menu::show(ui, section, rows, &pane.selected, busy) {
                            action = Some(label.into());
                        }
                    });
                }
                let mut x = body.left() + 15.;
                let grid_bottom = clip.bottom().min(body.top() + visible.len() as f32 * 24.);
                if grid_bottom > clip.top() {
                    for value in std::iter::once(&0.).chain(&widths) {
                        x += value;
                        painter.vline(
                            x - 0.5,
                            egui::Rangef::new(clip.top(), grid_bottom),
                            egui::Stroke::new(1., Color32::from_gray(224)),
                        );
                    }
                }
            };
            if bounded {
                egui::ScrollArea::vertical()
                    .id_salt("manager-rows")
                    .auto_shrink([false, false])
                    .show(ui, body);
            } else {
                body(ui);
            }
        });
    (chosen, action)
}
fn resize(
    ui: &mut Ui,
    header: egui::Rect,
    widths: &mut [f32],
    minimums: &[f32],
    saved: &mut Option<Box<[f32]>>,
) {
    if widths.len() < 2 {
        return;
    }
    let mut x = header.left() + 15.;
    for column in 0..widths.len() - 1 {
        x += widths[column];
        let handle =
            egui::Rect::from_center_size(egui::pos2(x, header.center().y), egui::vec2(7., 24.));
        let response = ui
            .interact(
                handle,
                ui.make_persistent_id(("manager-column", column)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let id = response.id.with("start");
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(id, [widths[column], widths[column + 1]]));
        }
        if response.dragged() {
            let start = ui
                .data(|d| d.get_temp::<[f32; 2]>(id))
                .unwrap_or([widths[column], widths[column + 1]]);
            let delta = response
                .total_drag_delta()
                .unwrap_or_default()
                .x
                .clamp(minimums[column] - start[0], start[1] - minimums[column + 1]);
            widths[column] = start[0] + delta;
            widths[column + 1] = start[1] - delta;
            *saved = Some(widths.to_vec().into_boxed_slice());
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<[f32; 2]>(id));
        }
    }
}
