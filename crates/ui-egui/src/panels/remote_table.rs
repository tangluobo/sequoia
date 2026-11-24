//! Shared virtual table geometry; protocol use cases remain outside the view.
use crate::row_selection::Selection;
use egui::{Color32, Ui, pos2, vec2};
use serde_json::Value;
#[cfg(test)]
mod tests;
pub fn text(v: &Value, key: &str) -> String {
    if matches!(
        key,
        "timestamp" | "storeTimestamp" | "bornTimestamp" | "lastUpdateTimestamp"
    ) && let Some(time) = v[key]
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
    {
        return time
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
    }
    tomato_services::cloud::text(v, key)
}
pub fn table(
    ui: &mut Ui,
    rows: &[Value],
    selected: &mut Option<usize>,
    columns: &[(&str, &str, f32)],
    height: f32,
    buttons: &[&str],
    menus: &[&str],
) -> Option<(usize, String)> {
    table_inner(ui, rows, selected, columns, height, buttons, menus, None)
}
#[allow(clippy::too_many_arguments)]
pub fn multi_table(
    ui: &mut Ui,
    rows: &[Value],
    selected: &mut Option<usize>,
    columns: &[(&str, &str, f32)],
    height: f32,
    menus: &[&str],
    selection: &mut Selection,
) -> Option<(usize, String)> {
    selection.rows.retain(|i| *i < rows.len());
    let focus = ui.make_persistent_id("remote-multiple-selection");
    ui.interact(
        ui.available_rect_before_wrap(),
        focus,
        egui::Sense::focusable_noninteractive(),
    );
    if ui.is_enabled()
        && ui.memory(|m| m.has_focus(focus))
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A))
    {
        selection.rows.extend(0..rows.len());
        if selected.is_none() {
            *selected = (!rows.is_empty()).then_some(0);
        }
    }
    table_inner(
        ui,
        rows,
        selected,
        columns,
        height,
        &[],
        menus,
        Some((selection, focus)),
    )
}
#[allow(clippy::too_many_arguments)]
fn table_inner(
    ui: &mut Ui,
    rows: &[Value],
    selected: &mut Option<usize>,
    columns: &[(&str, &str, f32)],
    height: f32,
    buttons: &[&str],
    menus: &[&str],
    mut selection: Option<(&mut Selection, egui::Id)>,
) -> Option<(usize, String)> {
    let mut event = None;
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.remove::<Vec<(usize, egui::Rect)>>(egui::Id::new("tomato-remote-visible-rows"));
    });
    let action_column = columns.iter().find(|(_, key, _)| *key == "@actions");
    let min_action_width = buttons
        .iter()
        .map(|label| action_button_width(label) + 4.)
        .sum::<f32>()
        + 4.;
    let preferred_width = columns.iter().map(|(_, _, w)| *w).sum::<f32>();
    let width = ui.available_width().max(if action_column.is_some() {
        min_action_width + 28. * (columns.len() - 1) as f32
    } else {
        1.
    });
    let preferred_action_width = action_column.map_or(0., |(_, _, w)| *w);
    let action_width = if action_column.is_some() {
        (width * preferred_action_width / preferred_width).max(min_action_width)
    } else {
        0.
    };
    let scale = (width - action_width) / (preferred_width - preferred_action_width).max(1.);
    let column_width = |key, w| {
        if key == "@actions" {
            action_width
        } else {
            w * scale
        }
    };
    let max_height = ui.available_height().max(60.);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .max_height(max_height)
        .show(ui, |ui| {
            ui.set_min_width(width);
            let (head, _) = ui.allocate_exact_size(vec2(width, 26.), egui::Sense::hover());
            ui.painter().rect_filled(head, 0., Color32::from_gray(246));
            let mut x = head.left();
            for (label, key, w) in columns {
                let r = egui::Rect::from_min_size(
                    pos2(x, head.top()),
                    vec2(column_width(*key, *w), 26.),
                );
                let mut job = egui::text::LayoutJob::simple_singleline(
                    (*label).to_owned(),
                    egui::FontId::proportional(12.),
                    Color32::from_gray(51),
                );
                job.wrap.max_width = (r.width() - 14.).max(0.);
                job.wrap.max_rows = 1;
                job.wrap.overflow_character = Some('…');
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                ui.painter()
                    .with_clip_rect(ui.clip_rect().intersect(r.shrink2(vec2(5., 0.))))
                    .galley(
                        pos2(r.left() + 7., r.center().y - galley.size().y / 2.),
                        galley,
                        Color32::from_gray(51),
                    );
                ui.painter().vline(
                    r.right(),
                    r.y_range(),
                    egui::Stroke::new(1., Color32::from_gray(225)),
                );
                x = r.right();
            }
            if rows.is_empty() {
                let empty = egui::Rect::from_min_size(
                    head.left_bottom(),
                    vec2(width, (max_height - 26.).max(0.)),
                );
                ui.painter().text(
                    empty.center(),
                    egui::Align2::CENTER_CENTER,
                    "无数据",
                    egui::FontId::proportional(12.),
                    Color32::BLACK,
                );
                ui.allocate_rect(empty, egui::Sense::hover());
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("remote-table-rows")
                .auto_shrink([false, false])
                .show_rows(ui, height, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    #[cfg(test)]
                    let mut visible = Vec::new();
                    for i in range {
                        let row = &rows[i];
                        let (r, _) =
                            ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                        let response =
                            ui.interact(r, ui.id().with(("remote-row", i)), egui::Sense::click());
                        #[cfg(test)]
                        visible.push((i, r.intersect(ui.clip_rect())));
                        ui.painter().rect_filled(
                            r,
                            0.,
                            if selection
                                .as_ref()
                                .map_or(*selected == Some(i), |(s, _)| s.rows.contains(&i))
                            {
                                Color32::from_rgb(232, 244, 255)
                            } else if i % 2 == 1 {
                                Color32::from_gray(250)
                            } else {
                                Color32::WHITE
                            },
                        );
                        let mut x = r.left();
                        for (_, key, w) in columns {
                            let cell = egui::Rect::from_min_size(
                                pos2(x, r.top()),
                                vec2(column_width(*key, *w), height),
                            );
                            let clip = ui.clip_rect().intersect(cell.shrink2(vec2(5., 0.)));
                            if *key == "@actions" {
                                let mut left = cell.left() + 4.;
                                for label in buttons {
                                    let button = egui::Rect::from_min_size(
                                        pos2(left, cell.center().y - 12.),
                                        vec2(action_button_width(label), 24.),
                                    );
                                    if ui.put(button, egui::Button::new(*label)).clicked() {
                                        event = Some((i, (*label).into()));
                                    }
                                    left = button.right() + 4.;
                                }
                            } else if *key == "ddns" {
                                let switch =
                                    egui::Rect::from_center_size(cell.center(), vec2(32., 18.));
                                let enabled = row["type"] == "A";
                                let active = row["ddns"].as_bool().unwrap_or(false);
                                let fill = if !enabled {
                                    Color32::from_gray(230)
                                } else if active {
                                    Color32::from_rgb(7, 193, 96)
                                } else {
                                    Color32::from_gray(190)
                                };
                                ui.painter().rect_filled(switch, 9., fill);
                                ui.painter().circle_filled(
                                    pos2(
                                        if active {
                                            switch.right() - 9.
                                        } else {
                                            switch.left() + 9.
                                        },
                                        switch.center().y,
                                    ),
                                    7.,
                                    Color32::WHITE,
                                );
                                if enabled
                                    && ui
                                        .interact(
                                            switch,
                                            ui.id().with(("ddns", i)),
                                            egui::Sense::click(),
                                        )
                                        .clicked()
                                {
                                    event = Some((i, "DDNS".into()));
                                }
                            } else {
                                ui.painter().with_clip_rect(clip).text(
                                    cell.left_center() + vec2(7., 0.),
                                    egui::Align2::LEFT_CENTER,
                                    text(row, key),
                                    egui::FontId::proportional(12.),
                                    if *key == "status"
                                        && text(row, key).eq_ignore_ascii_case("enable")
                                    {
                                        Color32::from_rgb(76, 175, 80)
                                    } else {
                                        Color32::from_gray(51)
                                    },
                                );
                            }
                            x = cell.right();
                        }
                        ui.painter().hline(
                            r.x_range(),
                            r.bottom(),
                            egui::Stroke::new(1., Color32::from_gray(235)),
                        );
                        if response.clicked() || response.secondary_clicked() {
                            if let Some((selection, focus)) = selection.as_mut() {
                                selection.select(
                                    i,
                                    selected,
                                    ui.input(|i| i.modifiers),
                                    response.secondary_clicked(),
                                );
                                ui.memory_mut(|m| m.request_focus(*focus));
                            } else {
                                *selected = Some(i);
                            }
                            if response.clicked() {
                                event = Some((i, "选择".into()));
                            }
                        }
                        if response.double_clicked() {
                            event = Some((i, "打开".into()));
                        }
                        if !menus.is_empty() {
                            response.context_menu(|ui| {
                                for label in menus {
                                    if ui.add(crate::compact_menu::Item::new(label)).clicked() {
                                        event = Some((i, (*label).into()));
                                        ui.close();
                                    }
                                }
                            });
                        }
                    }
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("tomato-remote-visible-rows"), visible)
                    });
                });
        });
    event
}
fn action_button_width(label: &str) -> f32 {
    if label == "申请证书" { 66. } else { 44. }
}
pub fn button(ui: &mut Ui, label: &str, green: bool) -> egui::Response {
    let mut b = egui::Button::new(egui::RichText::new(label).size(12.).color(if green {
        Color32::WHITE
    } else {
        Color32::from_gray(51)
    }));
    if green {
        b = b.fill(Color32::from_rgb(7, 193, 96));
    }
    ui.add_sized([label.chars().count() as f32 * 12. + 12., 24.], b)
}
