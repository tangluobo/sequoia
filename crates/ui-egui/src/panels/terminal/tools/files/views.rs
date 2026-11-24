use super::*;
use crate::tabs::ssh::Pane;
use tomato_services::terminal::ssh::tools::Entry;

pub(super) fn render(
    ui: &mut Ui,
    pane: &mut Pane,
    mode: &str,
    icons: &mut crate::icons::Icons,
    connection: &tomato_core::Connection,
) -> (Option<String>, Option<String>, Option<Operation>) {
    let (mut navigate, mut action, mut operation) = (None, None, None);
    let mut rename_rect = None;
    let mut export_entry = None;
    if mode == "COLUMN" {
        return super::columns::render(ui, pane, icons, connection);
    }
    {
        let filter = pane.filter.to_lowercase();
        let indices: Vec<_> = pane
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name.to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        let grid = super::grid::Grid::new(ui, ui.available_width());
        let columns = grid.columns;
        let (bounds, _) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                grid.content_height(indices.len())
                    .max(ui.available_height().max(0.)),
            ),
            egui::Sense::hover(),
        );
        pane.content_bounds = Some(bounds);
        let tile = |display| grid.cell(bounds, display);
        let (range, background) =
            super::selection::update(ui, pane, "ICON", bounds, &indices, tile);
        let first = ((ui.clip_rect().top() - bounds.top() - 12.) / grid.step.y)
            .floor()
            .max(0.) as usize
            * columns;
        let last = (((ui.clip_rect().bottom() - bounds.top() - 12.) / grid.step.y)
            .ceil()
            .max(0.) as usize
            * columns)
            .min(indices.len());
        for (display, &index) in indices.iter().enumerate().take(last).skip(first) {
            let entry = &pane.entries[index];
            let rect = tile(display);
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
            super::export::observe(ui, entry, &response);
            if !editing && pane.selected.contains(&index) {
                ui.painter().rect(
                    rect,
                    6,
                    Color32::from_rgb(231, 243, 255),
                    egui::Stroke::new(1., Color32::from_rgb(51, 153, 255)),
                    egui::StrokeKind::Inside,
                );
            }
            let response = if editing {
                response
            } else {
                response.on_hover_cursor(egui::CursorIcon::PointingHand)
            };
            let icon_rect = egui::Rect::from_center_size(
                rect.center_top() + egui::vec2(0., 30.),
                egui::vec2(48., 48.),
            );
            let thumbnail = if crate::tabs::ssh::images::supported_connection(connection) {
                pane.thumbnails.visible(entry)
            } else {
                None
            };
            if let Some(texture) = thumbnail {
                let size = texture.size_vec2();
                let scale = (icon_rect.width() / size.x).min(icon_rect.height() / size.y);
                ui.painter().image(
                    texture.id(),
                    egui::Rect::from_center_size(icon_rect.center(), size * scale),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
            } else {
                icon(
                    ui,
                    icons,
                    entry,
                    bucket_icon(connection, &pane.path),
                    icon_rect,
                );
            }
            let mut job = egui::text::LayoutJob::simple(
                entry.name.clone(),
                egui::FontId::proportional(11.),
                Color32::from_gray(51),
                82.,
            );
            job.wrap.max_rows = 1;
            job.wrap.overflow_character = Some('…');
            let text = ui.painter().layout_job(job);
            if editing {
                rename_rect = Some(egui::Rect::from_center_size(
                    rect.center_top() + egui::vec2(0., 65.),
                    egui::vec2(82., 22.),
                ));
            } else {
                ui.painter()
                    .with_clip_rect(rect.intersect(ui.clip_rect()))
                    .galley(
                        egui::pos2(rect.center().x - text.size().x / 2., rect.top() + 58.),
                        text,
                        Color32::from_gray(51),
                    );
            }
            rename::click(
                ui,
                &mut pane.rename,
                entry,
                &response,
                (&pane.path, &pane.filter, mode),
                pane.selected.len() == 1 && pane.selected.contains(&index),
                bucket_icon(connection, &pane.path).is_none(),
            );
            if response.clicked() || response.secondary_clicked() || response.drag_started() {
                response.request_focus();
                pane.focus = Some(response.id);
                pane.file_cursor = Some(index);
                pane.column_selected = None;
                if response.drag_started() && pane.selected.contains(&index) {
                    // A drag of an existing selection retains the complete file list.
                } else if ui
                    .input(|i| i.modifiers.shift && !i.modifiers.ctrl && !i.modifiers.mac_cmd)
                    && !response.secondary_clicked()
                {
                    pane.selected.insert(index);
                } else {
                    super::super::table::select(
                        &mut pane.selected,
                        &mut pane.anchor,
                        index,
                        ui.input(|i| i.modifiers),
                        response.secondary_clicked(),
                    );
                }
            }
            if response.drag_started() && !entry.directory {
                export_entry = Some((entry.clone(), response.clone()));
            }
            if response.double_clicked() {
                if entry.directory {
                    navigate = Some(entry.path.clone());
                } else {
                    action = Some("打开".into());
                }
            }
            response.context_menu(|ui| {
                if let Some(next) = super::menu::show(ui, pane, connection) {
                    action = Some(next);
                }
            });
        }
        super::menu::background(background, pane, connection, &mut action);
        super::selection::paint(ui, range, bounds);
        if let Some(rect) = rename_rect {
            operation = rename::field(ui, pane, rect, true);
        }
    }
    if let Some((entry, response)) = export_entry {
        operation = operation.or_else(|| super::export::row(ui, pane, &entry, &response));
    }
    (navigate, action, operation)
}

pub(super) fn icon(
    ui: &Ui,
    icons: &mut crate::icons::Icons,
    entry: &Entry,
    bucket: Option<&str>,
    rect: egui::Rect,
) {
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("TXT")
        .to_ascii_uppercase();
    let candidate = format!("images/connect/fileTypes/{ext}.png");
    let icon = if let Some(icon) = bucket {
        icon
    } else if entry.directory {
        "images/connect/folder.png"
    } else if crate::asset_bytes::bytes(&candidate).is_some() {
        &candidate
    } else {
        "images/connect/fileTypes/TXT.png"
    };
    icons.paint(ui, icon, rect);
}
