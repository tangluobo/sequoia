use super::*;
use crate::tabs::ssh::{FileExport, Pane};
/// Fixture-only hit routing evidence; never records ordinary browsing activity.
pub(super) fn observe(
    ui: &Ui,
    entry: &tomato_services::files::operations::Entry,
    response: &egui::Response,
) {
    if !ui.input(|i| {
        i.pointer.primary_pressed() || i.pointer.primary_released() || response.drag_started()
    }) || !ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("file-drop-preview-installed")))
        .unwrap_or(false)
        || !ui.input(|i| {
            i.pointer
                .interact_pos()
                .is_some_and(|p| response.rect.intersect(ui.clip_rect()).contains(p))
        })
    {
        return;
    }
    let event = ui.input(|i| serde_json::json!({"phase":"row-input","name":entry.name,"focused":i.focused,"primaryDown":i.pointer.primary_down(),"pressed":i.pointer.primary_pressed(),"released":i.pointer.primary_released(),"dragStarted":response.drag_started(),"downOnRow":response.is_pointer_button_down_on(),"hovered":response.hovered(),"enabled":response.enabled(),"time":i.time}));
    record(ui, event);
}
pub(super) fn row(
    ui: &Ui,
    pane: &mut Pane,
    entry: &tomato_services::files::operations::Entry,
    response: &egui::Response,
) -> Option<Operation> {
    if response.drag_started()
        && pane.rename.is_none()
        && !ui.input(|i| i.modifiers.alt || i.modifiers.shift)
    {
        let mut items = Vec::new();
        let selected = pane
            .entries
            .iter()
            .position(|e| e.path == entry.path)
            .is_some_and(|i| pane.selected.contains(&i));
        if selected {
            for &index in &pane.selected {
                if let Some(item) = pane.entries.get(index) {
                    items.push(tomato_services::files::export::Item::new(
                        item.path.clone(),
                        Some(item.size),
                        item.directory,
                    ));
                }
            }
        } else {
            if let Some(index) = pane.entries.iter().position(|e| e.path == entry.path) {
                pane.selected = [index].into();
                pane.anchor = Some(index);
                pane.file_cursor = Some(index);
            }
            items.push(tomato_services::files::export::Item::new(
                entry.path.clone(),
                Some(entry.size),
                entry.directory,
            ));
        }
        let request = match tomato_services::files::export::Request::selection(items) {
            Ok(request) => request,
            Err(error) => {
                pane.status = format!("文件拖出失败：{error:#}");
                return None;
            }
        };
        if let Some(old) = pane.export.take() {
            old.cancel.cancel();
        }
        pane.export = Some(Box::new(FileExport {
            path: request.path.clone(),
            cancel: request.cancel.clone(),
            asset: None,
            gesture: true,
            request: request.clone(),
            native: None,
        }));
        trace(ui, "prepare", &request);
        ui.ctx().request_repaint();
        return Some(Operation::ExportFile(request));
    }
    None
}
pub(super) fn poll(ui: &Ui, pane: &mut Pane) {
    let Some(export) = &mut pane.export else {
        return;
    };
    if let Some(error) = export.request.failure() {
        pane.status = format!("文件下载失败：{error}");
        export.cancel.cancel();
        pane.export = None;
        return;
    }
    if let Some(native) = &export.native {
        if ui
            .ctx()
            .data(|d| d.get_temp::<bool>(egui::Id::new("file-drop-preview-installed")))
            .unwrap_or(false)
        {
            let state = native.diagnostics();
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("file-export-native-state"), state));
        }
        if let Some(result) = native.try_result() {
            pane.status = if let Some(error) = export.request.failure() {
                format!("文件下载失败：{error}")
            } else {
                match result {
                    Ok(true) => "文件拖放完成".into(),
                    Ok(false) => "文件拖放已取消".into(),
                    Err(error) => format!("系统文件拖放失败：{error:#}"),
                }
            };
            trace(ui, &pane.status, &export.request);
            pane.export = None;
            ui.ctx()
                .input_mut(|input| input.pointer = egui::PointerState::default());
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
        }
        return;
    }
    if !ui.input(|i| i.focused && i.pointer.primary_down() && !i.key_pressed(egui::Key::Escape)) {
        trace(
            ui,
            if export.asset.is_some() {
                "cancel-ready"
            } else {
                "cancel-pending"
            },
            &export.request,
        );
        export.cancel.cancel();
        pane.status = "文件拖放已取消".into();
        pane.export = None;
        return;
    }
    if !export.request.started() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(10));
        return;
    }
    if export.gesture && ui.ctx().native_pixels_per_point().is_some() {
        trace(ui, "native-start", &export.request);
        export.gesture = false;
        match tomato_services::files::export::begin(export.request.clone()) {
            Ok(native) => {
                export.native = Some(native);
                ui.ctx().request_repaint();
            }
            Err(error) => {
                pane.status = format!("系统文件拖放失败：{error:#}");
                pane.export = None;
            }
        }
    }
}
fn trace(ui: &Ui, phase: &str, request: &tomato_services::files::export::Request) {
    if !ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("file-drop-preview-installed")))
        .unwrap_or(false)
    {
        return;
    }
    let view = ui
        .ctx()
        .data(|d| d.get_temp::<String>(egui::Id::new("file-export-view")));
    let files: Vec<_> = request
        .items()
        .iter()
        .map(|item| item.path.rsplit('/').next().unwrap_or_default())
        .collect();
    let event = ui.input(|i| serde_json::json!({"phase":phase,"focused":i.focused,"primaryDown":i.pointer.primary_down(),"time":i.time,"view":view,"files":files}));
    record(ui, event);
}
fn record(ui: &Ui, event: serde_json::Value) {
    ui.data_mut(|data| {
        let events = data
            .get_temp_mut_or_default::<Vec<serde_json::Value>>(egui::Id::new("file-export-events"));
        if events.len() == 64 {
            events.remove(0);
        }
        events.push(event);
    });
}
