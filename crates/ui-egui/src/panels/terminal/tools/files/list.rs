//! Virtual file details with fixed headers and constrained column resizing.
use super::*;
use crate::tabs::ssh::Pane;
#[derive(Default)]
pub(super) struct Output {
    pub action: Option<String>,
    pub navigate: Option<String>,
    pub rename_operation: Option<Operation>,
    pub export_request: Option<Operation>,
    pub sort_request: Option<usize>,
}
pub(super) fn render(
    ui: &mut Ui,
    pane: &mut Pane,
    icons: &mut crate::icons::Icons,
    connection: &tomato_core::Connection,
    standalone: bool,
    bucket_root: bool,
    indices: &[usize],
) -> Output {
    let row_h = 26.;
    let mut widths = super::header::widths(pane, ui.available_width(), standalone);
    let mut out = Output::default();
    let mut rename_rect = None;
    egui::ScrollArea::horizontal()
        .id_salt("files-columns")
        .show(ui, |ui| {
            let (bounds, _background) = ui.allocate_exact_size(
                egui::vec2(
                    if standalone {
                        widths.iter().sum::<f32>().max(ui.available_width())
                    } else {
                        ui.available_width()
                    },
                    (row_h * (indices.len() + 1) as f32).max(if standalone {
                        ui.available_height().max(80.)
                    } else {
                        80.
                    }),
                ),
                egui::Sense::click(),
            );
            pane.content_bounds = Some(bounds);
            let clip = ui.clip_rect();
            let header_top = bounds.top().max(clip.top());
            paint_columns(
                ui,
                header_top,
                bounds.left(),
                &[
                    "名称".into(),
                    "大小".into(),
                    "修改时间".into(),
                    "类型".into(),
                ],
                &widths,
                true,
            );
            let mut header_x = bounds.left();
            for (column, width) in widths.iter().enumerate() {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(header_x, header_top),
                    egui::vec2(*width, row_h),
                );
                if ui
                    .interact(
                        rect.shrink2(egui::vec2(4., 0.)),
                        ui.make_persistent_id(("file-sort", column)),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    out.sort_request = Some(column);
                }
                header_x += width;
            }
            super::header::resize(ui, pane, &mut widths, bounds.left(), header_top);
            ui.set_clip_rect(clip.intersect(egui::Rect::from_min_max(
                egui::pos2(clip.left(), header_top + row_h),
                clip.max,
            )));
            let mut content_widths = widths.clone();
            content_widths[0] -= 20.;
            let selection_bounds =
                egui::Rect::from_min_max(bounds.min + egui::vec2(0., row_h), bounds.max);
            let (range, background) =
                selection::update(ui, pane, "LIST", selection_bounds, indices, |display| {
                    egui::Rect::from_min_size(
                        selection_bounds.min + egui::vec2(0., display as f32 * row_h),
                        egui::vec2(bounds.width(), row_h),
                    )
                });
            let start = ((ui.clip_rect().top() - bounds.top()) / row_h)
                .floor()
                .max(1.) as usize
                - 1;
            let end = (((ui.clip_rect().bottom() - bounds.top()) / row_h)
                .ceil()
                .max(1.) as usize)
                .min(indices.len());
            for (display, &index) in indices.iter().enumerate().take(end).skip(start) {
                let entry = &pane.entries[index];
                let rect = egui::Rect::from_min_size(
                    egui::pos2(bounds.left(), bounds.top() + (display + 1) as f32 * row_h),
                    egui::vec2(bounds.width(), row_h),
                );
                let editing = rename::editing(&pane.rename, &entry.path);
                let response = ui.interact(
                    rect,
                    ui.make_persistent_id(index),
                    if editing {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click_and_drag()
                    },
                );
                export::observe(ui, entry, &response);
                if !editing && (pane.selected.contains(&index) || response.hovered()) {
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
                let ext = std::path::Path::new(&entry.name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("TXT")
                    .to_ascii_uppercase();
                let candidate = format!("images/connect/fileTypes/{ext}.png");
                let icon = if let Some(icon) = bucket_icon(connection, &pane.path) {
                    icon
                } else if entry.directory {
                    "images/connect/folder.png"
                } else if crate::asset_bytes::bytes(&candidate).is_some() {
                    &candidate
                } else {
                    "images/connect/fileTypes/TXT.png"
                };
                icons.paint(
                    ui,
                    icon,
                    egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(10., 0.),
                        egui::vec2(16., 16.),
                    ),
                );
                paint_columns(
                    ui,
                    rect.top(),
                    rect.left() + 20.,
                    &[
                        if editing {
                            String::new()
                        } else if entry.directory && !bucket_root {
                            format!("{}/", entry.name)
                        } else {
                            entry.name.clone()
                        },
                        if bucket_root {
                            String::new()
                        } else if entry.directory {
                            "<DIR>".into()
                        } else {
                            file_size(entry.size)
                        },
                        entry.modified_text(),
                        if bucket_root {
                            "Bucket".into()
                        } else if entry.directory {
                            "目录".into()
                        } else {
                            "文件".into()
                        },
                    ],
                    &content_widths,
                    false,
                );
                rename::click(
                    ui,
                    &mut pane.rename,
                    entry,
                    &response,
                    (&pane.path, &pane.filter, "LIST"),
                    pane.selected.len() == 1 && pane.selected.contains(&index),
                    !bucket_root,
                );
                if response.drag_started() {
                    let entry = entry.clone();
                    out.export_request = export::row(ui, pane, &entry, &response);
                }
                let entry = &pane.entries[index];
                if response.clicked() || response.secondary_clicked() {
                    response.request_focus();
                    pane.focus = Some(response.id);
                    pane.file_cursor = Some(index);
                    pane.column_selected = None;
                    super::super::table::select(
                        &mut pane.selected,
                        &mut pane.anchor,
                        index,
                        ui.input(|i| i.modifiers),
                        response.secondary_clicked(),
                    );
                }
                if editing {
                    rename_rect = Some(egui::Rect::from_min_size(
                        rect.min + egui::vec2(20., 0.),
                        egui::vec2(widths[0] - 20., row_h),
                    ));
                }
                if response.double_clicked() {
                    if entry.directory {
                        out.navigate = Some(entry.path.clone());
                    } else {
                        out.action = Some("打开".into());
                    }
                }
                response.context_menu(|ui| {
                    if let Some(next) = menu::show(ui, pane, connection) {
                        out.action = Some(next);
                    }
                });
            }
            if background.clicked() {
                if pane.rename.as_ref().is_some_and(|edit| edit.due.is_some()) {
                    pane.finish_file_rename(ui.ctx(), false);
                }
                background.request_focus();
                pane.focus = Some(background.id);
            }
            menu::background(background, pane, connection, &mut out.action);
            if indices.is_empty() {
                ui.painter().text(
                    bounds.center(),
                    egui::Align2::CENTER_CENTER,
                    "空目录",
                    egui::FontId::proportional(12.),
                    Color32::GRAY,
                );
            }
            selection::paint(ui, range, selection_bounds);
            if let Some(rect) = rename_rect {
                out.rename_operation = rename::field(ui, pane, rect, false);
            }
            ui.set_clip_rect(clip);
        });

    out
}
