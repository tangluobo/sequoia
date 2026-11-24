//! Java column browser: 220-point columns, independent scrolling and retained ancestors.
use super::*;
use crate::tabs::ssh::Pane;
const WIDTH: f32 = 220.;
const ROW: f32 = 26.;
pub(super) fn render(
    ui: &mut Ui,
    pane: &mut Pane,
    icons: &mut crate::icons::Icons,
    connection: &tomato_core::Connection,
) -> (Option<String>, Option<String>, Option<Operation>) {
    let (mut navigate, mut action, mut operation) = (None, None, None);
    let mut export_entry = None;
    let height = ui.available_height().max(200.);
    egui::ScrollArea::horizontal()
        .id_salt("sftp-browser-columns")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let count = pane.columns.len() + 1;
            let (bounds, _) = ui.allocate_exact_size(
                egui::vec2((count as f32 * WIDTH).max(ui.available_width()), height),
                egui::Sense::hover(),
            );
            let filter = pane.filter.to_lowercase();
            for column in 0..count {
                let current = column == pane.columns.len();
                let path = if current {
                    pane.path.clone()
                } else {
                    pane.columns[column].0.clone()
                };
                let indices: Vec<_> = if current {
                    pane.entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.name.to_lowercase().contains(&filter))
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    (0..pane.columns[column].1.len()).collect()
                };
                let rect = egui::Rect::from_min_size(
                    bounds.min + egui::vec2(column as f32 * WIDTH, 0.),
                    egui::vec2(WIDTH, height),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("file-column", &path))
                        .max_rect(rect),
                );
                child.set_clip_rect(rect.intersect(ui.clip_rect()));
                child.painter().vline(
                    rect.right(),
                    rect.y_range(),
                    egui::Stroke::new(1., Color32::from_gray(229)),
                );
                let keyboard_focus = child.interact(
                    rect,
                    child.make_persistent_id("column-focus"),
                    egui::Sense::click(),
                );
                let active = pane
                    .focus
                    .is_some_and(|id| child.memory(|m| m.has_focus(id)))
                    && pane
                        .column_selected
                        .as_deref()
                        .map(|selected| parent(selected) == path)
                        .unwrap_or(current);
                let mut reveal = None;
                if active && pane.rename.is_none() && !egui::Popup::is_any_open(child.ctx()) {
                    if current {
                        reveal = keyboard::update(
                            &child,
                            pane,
                            &indices,
                            1,
                            (height / ROW).floor().max(1.) as usize,
                            false,
                        );
                        if let Some(display) = reveal {
                            pane.column_selected =
                                Some(pane.entries[indices[display]].path.clone());
                        }
                    } else {
                        let old = pane.column_selected.as_deref().and_then(|selected| {
                            pane.columns[column]
                                .1
                                .iter()
                                .position(|e| e.path == selected)
                        });
                        if let Some(display) = keyboard::target(
                            &child,
                            old,
                            indices.len(),
                            1,
                            (height / ROW).floor().max(1.) as usize,
                            false,
                        ) {
                            pane.column_selected =
                                Some(pane.columns[column].1[indices[display]].path.clone());
                            pane.selected.clear();
                            pane.anchor = None;
                            reveal = Some(display);
                        }
                    }
                    if let Some(display) = reveal {
                        keyboard_focus.request_focus();
                        pane.focus = Some(keyboard_focus.id);
                        let entry = if current {
                            &pane.entries[indices[display]]
                        } else {
                            &pane.columns[column].1[indices[display]]
                        };
                        if entry.directory {
                            navigate = Some(entry.path.clone());
                        }
                    }
                }
                let mut area = egui::ScrollArea::vertical()
                    .id_salt(("file-column-scroll", &path))
                    .auto_shrink([false, false])
                    .max_height(height);
                if let Some(display) = reveal {
                    area = area.vertical_scroll_offset(display as f32 * ROW);
                }
                let scroll_id =
                    child.make_persistent_id(egui::IdSalt::new(("file-column-scroll", &path)));
                let offset = egui::scroll_area::State::load(child.ctx(), scroll_id)
                    .map(|s| s.offset.y)
                    .unwrap_or(0.);
                let selection_bounds = egui::Rect::from_min_size(
                    rect.min - egui::vec2(0., offset),
                    egui::vec2(
                        (bounds.right() - rect.left()).max(WIDTH),
                        (indices.len() as f32 * ROW).max(height),
                    ),
                );
                let clip = child.clip_rect();
                child.set_clip_rect(
                    egui::Rect::from_min_max(rect.min, egui::pos2(bounds.right(), rect.bottom()))
                        .intersect(ui.clip_rect()),
                );
                let (range, range_background) = if current {
                    let (range, response) = selection::update_in_column(
                        &child,
                        pane,
                        selection_bounds,
                        &indices,
                        |display| {
                            egui::Rect::from_min_size(
                                selection_bounds.min + egui::vec2(0., display as f32 * ROW),
                                egui::vec2(WIDTH, ROW),
                            )
                        },
                    );
                    (range, Some(response))
                } else {
                    (None, None)
                };
                if current
                    && pane.file_drag.is_some()
                    && child.input(|i| i.pointer.primary_down())
                    && let Some(pointer) = child.input(|i| i.pointer.interact_pos())
                {
                    let dy = if pointer.y > rect.bottom() - 25. {
                        8.
                    } else if pointer.y < rect.top() + 25. {
                        -8.
                    } else {
                        0.
                    };
                    if dy != 0. {
                        area = area.vertical_scroll_offset((offset + dy).max(0.));
                        child
                            .ctx()
                            .request_repaint_after(std::time::Duration::from_millis(16));
                    }
                }
                child.set_clip_rect(clip);
                area.show(&mut child, |ui| {
                    let (content, background) = ui.allocate_exact_size(
                        egui::vec2(
                            ui.available_width(),
                            (indices.len() as f32 * ROW).max(height),
                        ),
                        egui::Sense::click(),
                    );
                    let background = if let Some(response) = &range_background {
                        background.union(response.clone())
                    } else {
                        background
                    };
                    let first = ((ui.clip_rect().top() - content.top()) / ROW)
                        .floor()
                        .max(0.) as usize;
                    let last = (((ui.clip_rect().bottom() - content.top()) / ROW)
                        .ceil()
                        .max(0.) as usize)
                        .min(indices.len());
                    let entries = if current {
                        &pane.entries
                    } else {
                        &pane.columns[column].1
                    };
                    let mut rename_rect = None;
                    for (display, &index) in indices.iter().enumerate().take(last).skip(first) {
                        let entry = &entries[index];
                        let row = egui::Rect::from_min_size(
                            content.min + egui::vec2(0., display as f32 * ROW),
                            egui::vec2(content.width(), ROW),
                        );
                        let editing = rename::editing(&pane.rename, &entry.path);
                        let response = ui.interact(
                            row,
                            ui.make_persistent_id(("entry", &entry.path)),
                            if editing {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click_and_drag()
                            },
                        );
                        if !editing
                            && (current && pane.selected.contains(&index)
                                || pane.column_selected.as_deref() == Some(&entry.path)
                                || response.hovered())
                        {
                            ui.painter()
                                .rect_filled(row, 0, crate::theme::TREE_SELECTED);
                        }
                        views::icon(
                            ui,
                            icons,
                            entry,
                            bucket_icon(connection, &path),
                            egui::Rect::from_center_size(
                                row.left_center() + egui::vec2(12., 0.),
                                egui::vec2(16., 16.),
                            ),
                        );
                        if editing {
                            rename_rect = Some(egui::Rect::from_min_max(
                                row.min + egui::vec2(25., 0.),
                                row.max - egui::vec2(3., 0.),
                            ));
                        } else {
                            let name = if entry.directory {
                                format!("{}/", entry.name)
                            } else {
                                entry.name.clone()
                            };
                            let mut job = egui::text::LayoutJob::simple(
                                name,
                                egui::FontId::proportional(12.),
                                Color32::from_gray(51),
                                (row.width() - if entry.directory { 45. } else { 29. }).max(1.),
                            );
                            job.wrap.max_rows = 1;
                            job.wrap.overflow_character = Some('…');
                            let text = ui.painter().layout_job(job);
                            ui.painter().galley(
                                row.left_center() + egui::vec2(25., -text.size().y / 2.),
                                text,
                                Color32::from_gray(51),
                            );
                            if entry.directory {
                                ui.painter().text(
                                    row.right_center() - egui::vec2(12., 0.),
                                    egui::Align2::CENTER_CENTER,
                                    "›",
                                    egui::FontId::proportional(16.),
                                    Color32::from_gray(153),
                                );
                            }
                        }
                        export::observe(ui, entry, &response);
                        let single = if current && !pane.selected.is_empty() {
                            pane.selected.len() == 1 && pane.selected.contains(&index)
                        } else {
                            pane.column_selected.as_deref() == Some(&entry.path)
                        };
                        let scheduled = rename::click(
                            ui,
                            &mut pane.rename,
                            entry,
                            &response,
                            (&pane.path, &pane.filter, "COLUMN"),
                            single,
                            bucket_icon(connection, &path).is_none(),
                        );
                        if response.clicked()
                            || response.secondary_clicked()
                            || response.drag_started()
                        {
                            response.request_focus();
                            pane.focus = Some(response.id);
                            if current {
                                pane.file_cursor = Some(index);
                                if !(response.drag_started() && pane.selected.contains(&index)) {
                                    super::super::table::select(
                                        &mut pane.selected,
                                        &mut pane.anchor,
                                        index,
                                        ui.input(|i| i.modifiers),
                                        response.secondary_clicked(),
                                    );
                                }
                            } else {
                                pane.selected.clear();
                                pane.anchor = None;
                            }
                            pane.column_selected = Some(entry.path.clone());
                            if response.clicked() && entry.directory && !scheduled {
                                navigate = Some(entry.path.clone());
                            }
                        }
                        if response.drag_started() && !entry.directory {
                            export_entry = Some((entry.clone(), response.clone()));
                        }
                        if response.double_clicked() && !entry.directory {
                            action = Some("打开".into());
                        }
                        response.context_menu(|ui| {
                            if let Some(next) = menu::show(ui, pane, connection) {
                                action = Some(next);
                            }
                        });
                    }
                    menu::background(background, pane, connection, &mut action);
                    if let Some(rect) = rename_rect {
                        operation = rename::field(ui, pane, rect, false);
                    }
                });
                child.set_clip_rect(
                    egui::Rect::from_min_max(rect.min, egui::pos2(bounds.right(), rect.bottom()))
                        .intersect(ui.clip_rect()),
                );
                selection::paint(&child, range, selection_bounds);
            }
        });
    if let Some((entry, response)) = export_entry {
        operation = operation.or_else(|| export::row(ui, pane, &entry, &response));
    }
    (navigate, action, operation)
}
