mod actions;
mod columns;
#[cfg(test)]
mod copy_tests;
mod drop;
#[cfg(test)]
mod drop_tests;
mod export;
mod grid;
mod header;
mod keyboard;
mod list;
mod menu;
#[cfg(test)]
mod menu_tests;
mod rename;
#[cfg(test)]
mod rename_tests;
mod selection;
#[cfg(test)]
mod selection_tests;
mod toolbar;
mod views;
use super::dialogs::confirm;
use super::*;
pub(crate) use crate::tabs::ssh::{join, parent};
pub(crate) fn render(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let keyboard_focus = ui.interact(
        ui.available_rect_before_wrap(),
        ui.make_persistent_id("file-browser-keyboard-focus"),
        egui::Sense::click(),
    );
    let section = Section::Files;
    let top = ui.cursor().min;
    let rename_operation;
    let mut sort_request = None;
    let mut export_request = None;
    let object = tomato_services::files::object_copy::is_object(connection);
    let mut copy_shortcut = false;
    let standalone = workspace.standalone == Some(Section::Files);
    let bucket_root = bucket_icon(
        connection,
        workspace.panes.entry(section).or_default().path.as_str(),
    )
    .is_some();
    let (mut action, mut navigate) = if workspace.external_file_toolbar {
        (None, None)
    } else {
        toolbar::render(ui, workspace, connection, icons)
    };
    let mode = workspace.file_view.clone();
    if ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("file-drop-preview-installed")))
        .unwrap_or(false)
    {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("file-export-view"), mode.clone()));
    }
    let pane = workspace.panes.entry(section).or_default();
    rename::prepare(ui, pane, &mode);
    pane.thumbnails
        .set_mode(mode == "ICON" && crate::tabs::ssh::images::supported_connection(connection));
    let filter = pane.filter.to_lowercase();
    let indices: Vec<_> = pane
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.to_lowercase().contains(&filter))
        .map(|(i, _)| i)
        .collect();
    if mode == "LIST" {
        let list = list::render(
            ui,
            pane,
            icons,
            connection,
            standalone,
            bucket_root,
            &indices,
        );
        action = action.or(list.action);
        navigate = navigate.or(list.navigate);
        rename_operation = list.rename_operation;
        export_request = list.export_request;
        sort_request = list.sort_request;
    } else {
        let (next, chosen, renamed) = views::render(ui, pane, &mode, icons, connection);
        navigate = navigate.or(next);
        action = action.or(chosen);
        rename_operation = renamed;
    }
    rename::finish_frame(ui, pane);
    export::poll(ui, pane);
    if workspace.dialog.is_none() && pane.focus.is_some_and(|id| ui.memory(|m| m.has_focus(id))) {
        let grid = grid::Grid::new(
            ui,
            pane.content_bounds
                .map_or(ui.available_width(), |bounds| bounds.width()),
        );
        let columns = if mode == "ICON" { grid.columns } else { 1 };
        if mode != "COLUMN"
            && let Some(ordinal) = keyboard::update(
                ui,
                pane,
                &indices,
                columns,
                ((ui.clip_rect().height() / if mode == "ICON" { grid.step.y } else { 26. })
                    .floor()
                    .max(1.) as usize)
                    * columns,
                mode == "ICON",
            )
        {
            keyboard_focus.request_focus();
            pane.focus = Some(keyboard_focus.id);
            if let Some(bounds) = pane.content_bounds {
                let point = bounds.min
                    + if mode == "ICON" {
                        grid.cell(bounds, ordinal).min - bounds.min
                    } else {
                        egui::vec2(0., (ordinal + 1) as f32 * 26.)
                    };
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        point,
                        egui::vec2(40., if mode == "ICON" { grid.step.y } else { 26. }),
                    ),
                    None,
                );
            }
        }
        if object {
            copy_shortcut = ui.input_mut(|input| {
                let key = input.consume_key(egui::Modifiers::CTRL, egui::Key::C)
                    || input.consume_key(egui::Modifiers::COMMAND, egui::Key::C);
                let copied = input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy));
                if copied {
                    input
                        .events
                        .retain(|event| !matches!(event, egui::Event::Copy));
                }
                key || copied
            });
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            action = Some("刷新".into());
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            action = Some("重命名".into());
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            action = Some("删除".into());
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            action = Some("打开".into());
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
            navigate = Some(parent(&pane.path));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
            pane.selected = indices.iter().copied().collect();
        }
        // Backends may deliver Paste, a raw Ctrl+V, or only its key release.
        // Remember handling the press so a fast completed job is not repeated.
        let pressed = ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::CTRL, egui::Key::V)
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::V)
        });
        let pasted = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        });
        let released=ui.input(|i|i.events.iter().any(|event|matches!(event,egui::Event::Key{key:egui::Key::V,pressed:false,modifiers,..}if modifiers.ctrl||modifiers.mac_cmd)));
        let any_release = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed: false,
                        ..
                    }
                )
            })
        });
        if pasted || pressed {
            pane.paste_shortcut_active = true;
            action = Some("粘贴上传".into());
        }
        if any_release {
            if released && !pane.paste_shortcut_active {
                action = Some("粘贴上传".into());
            }
            pane.paste_shortcut_active = false;
        }
    } else {
        pane.paste_shortcut_active = false;
    }
    if let Some(column) = sort_request {
        pane.finish_file_rename(ui.ctx(), false);
        pane.sort_files(column);
    }
    let path = pane.path.clone();
    let selected: Vec<_> = if copy_shortcut {
        if mode == "COLUMN" && pane.selected.is_empty() {
            pane.column_selected
                .as_deref()
                .and_then(|path| pane.file_entry(path))
                .cloned()
                .into_iter()
                .collect()
        } else {
            pane.selected
                .iter()
                .filter_map(|i| pane.entries.get(*i).cloned())
                .collect()
        }
    } else {
        Vec::new()
    };
    if copy_shortcut && !selected.is_empty() {
        workspace.enqueue(
            section,
            Operation::CopyObjects {
                directory: if mode == "COLUMN" && selected.len() == 1 {
                    parent(&selected[0].path)
                } else {
                    path.clone()
                },
                paths: selected
                    .iter()
                    .map(|e| (e.path.clone(), e.directory))
                    .collect(),
            },
        );
    }
    if let Some(operation) = export_request {
        workspace.enqueue(section, operation);
    }
    if let Some(operation) = rename_operation {
        workspace.enqueue(section, operation);
    }
    let drop_rect = egui::Rect::from_min_max(top, ui.min_rect().max).intersect(ui.clip_rect());
    drop::receive(ui, workspace, connection, drop_rect, path.clone());
    actions::perform(ui, workspace, connection, action, navigate)
}

fn bucket_icon(connection: &tomato_core::Connection, path: &str) -> Option<&'static str> {
    let kind = if connection.kind() == "LOCAL_DIRECTORY" && connection.text("directoryType") == "S3"
    {
        "S3"
    } else {
        connection.kind()
    };
    (matches!(kind, "S3" | "ALIYUN_OSS")
        && connection.text("bucket").is_empty()
        && path.trim_matches('/').is_empty())
    .then(|| crate::icons::connection(kind))
}
fn access_url(connection: &tomato_core::Connection, path: &str) -> Option<String> {
    let base = connection
        .text("publicAccessUrl")
        .trim()
        .trim_end_matches('/');
    if base.is_empty()
        || !(matches!(connection.kind(), "S3" | "ALIYUN_OSS")
            || connection.kind() == "LOCAL_DIRECTORY" && connection.text("directoryType") == "S3")
    {
        return None;
    }
    let path = path.trim_start_matches('/');
    let (bucket, key) = if connection.text("bucket").is_empty() {
        path.split_once('/')?
    } else {
        (connection.text("bucket"), path)
    };
    let prefix = connection.text("s3Prefix").trim_matches('/');
    let key = if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}/{key}")
    };
    Some(format!("{base}/{bucket}/{key}"))
}

fn file_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = "KB";
    for next in ["MB", "GB"] {
        if value < 1024. {
            break;
        }
        value /= 1024.;
        unit = next;
    }
    format!("{value:.1} {unit}")
}
fn paint_columns(ui: &Ui, y: f32, mut x: f32, values: &[String], widths: &[f32], header: bool) {
    for (index, (value, width)) in values.iter().zip(widths).enumerate() {
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(*width, 26.));
        if header {
            ui.painter().rect_filled(rect, 0, Color32::from_gray(245));
            ui.painter().vline(
                rect.right(),
                rect.y_range(),
                egui::Stroke::new(1., Color32::from_gray(218)),
            );
            ui.painter().hline(
                rect.x_range(),
                rect.bottom(),
                egui::Stroke::new(1., Color32::from_gray(204)),
            );
        }
        let (point, align) = if header {
            (rect.center(), egui::Align2::CENTER_CENTER)
        } else if index == 1 {
            (
                rect.right_center() - egui::vec2(6., 0.),
                egui::Align2::RIGHT_CENTER,
            )
        } else {
            (
                rect.left_center() + egui::vec2(5., 0.),
                egui::Align2::LEFT_CENTER,
            )
        };
        ui.painter()
            .with_clip_rect(rect.intersect(ui.clip_rect()))
            .text(
                point,
                align,
                value,
                egui::FontId::proportional(if header || widths.len() == 4 {
                    12.
                } else {
                    11.
                }),
                Color32::from_gray(51),
            );
        x += width;
    }
}
fn nav_button(
    ui: &mut Ui,
    icons: &mut crate::icons::Icons,
    name: &str,
    title: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::click());
    icons.paint(
        ui,
        name,
        egui::Rect::from_center_size(rect.center(), egui::vec2(16., 16.)),
    );
    response.on_hover_text(title)
}

pub(crate) fn header(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let (action, navigate) = toolbar::render(ui, workspace, connection, icons);
    actions::perform(ui, workspace, connection, action, navigate)
}
