//! Original StoragePanel: fixed toolbar/footer, block table and LVM tree.
use super::*;
use crate::tabs::ssh::storage::Kind;
mod devices;
mod input;
#[cfg(test)]
mod tests;
mod tree;
pub(super) fn render(ui: &mut Ui, workspace: &mut Workspace) {
    notice(ui, workspace);
    ui.painter()
        .rect_filled(ui.max_rect(), 0, Color32::from_gray(244));
    let bounds = ui.max_rect().shrink(8.);
    let busy = workspace.busy(Section::Storage);
    let toolbar = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 23.));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(toolbar));
    bar.spacing_mut().item_spacing.x = 8.;
    bar.spacing_mut().interact_size.y = 23.;
    bar.spacing_mut().button_padding = egui::vec2(8., 3.);
    let mut action = None;
    bar.horizontal(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if ui.button("刷新").clicked() {
                workspace.panes.entry(Section::Storage).or_default().refresh = true;
            }
            for (label, kind) in [
                ("新硬盘初始化为 PV", Form::InitializePv),
                ("创建/扩展 VG", Form::VolumeGroup),
                ("扩展 LV", Form::ExtendLv),
                ("格式化/挂载", Form::Mount),
            ] {
                if ui.button(label).clicked() {
                    action = Some(kind);
                }
            }
        });
    });
    let footer = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), (bounds.bottom() - 21.).max(toolbar.bottom())),
        bounds.max,
    );
    let hint_y = footer.top() - 15.;
    let tree_height = ((hint_y - toolbar.bottom() - 88.) * 0.45).clamp(36., 220.);
    let tree_rect = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), hint_y - tree_height - 8.),
        egui::pos2(bounds.right(), hint_y - 8.),
    );
    let label_y = tree_rect.top() - 23.;
    let table_rect = egui::Rect::from_min_max(
        egui::pos2(bounds.left(), toolbar.bottom() + 8.),
        egui::pos2(bounds.right(), label_y - 8.),
    );
    if let Some(kind) = devices::render(ui, table_rect, workspace, busy) {
        action = Some(kind);
    }
    ui.painter().text(
        egui::pos2(bounds.left(), label_y),
        egui::Align2::LEFT_TOP,
        "LVM 关系（VG → PV / LV）",
        crate::theme::java_font(12.),
        Color32::from_gray(51),
    );
    if let Some(kind) = tree::render(ui, tree_rect, &mut workspace.storage, busy) {
        action = Some(kind);
    }
    ui.painter().text(
        egui::pos2(bounds.left(), hint_y),
        egui::Align2::LEFT_TOP,
        "提示：双击未分区磁盘可初始化；双击 PV 可加入 VG；双击 LV 可扩容",
        crate::theme::java_font(11.),
        Color32::from_gray(102),
    );
    let status = workspace
        .panes
        .get(&Section::Storage)
        .map_or("就绪", |p| p.status.as_str());
    ui.painter().with_clip_rect(footer).text(
        footer.left_center() + egui::vec2(2., 3.),
        egui::Align2::LEFT_CENTER,
        status,
        crate::theme::java_font(12.),
        Color32::from_gray(51),
    );
    if let Some(kind) = action {
        workspace.dialog = Some(draft(workspace, kind));
    }
}
fn draft(workspace: &Workspace, kind: Form) -> Dialog {
    let node = workspace.storage.selected_node();
    let table_path = workspace.panes.get(&Section::Storage).and_then(|p| {
        let index = p.selected.first()?;
        p.table
            .as_ref()?
            .rows
            .get(*index)?
            .first()
            .map(String::as_str)
    });
    let path = match kind {
        Form::VolumeGroup => node
            .filter(|n| n.kind == Kind::Pv)
            .map_or("/dev/sdb1", |n| n.path.as_str()),
        Form::ExtendLv => node
            .filter(|n| n.kind == Kind::Lv)
            .map_or("/dev/vg_data/lv_data", |n| n.path.as_str()),
        _ => table_path
            .or_else(|| node.filter(|n| !n.path.is_empty()).map(|n| n.path.as_str()))
            .unwrap_or("/dev/"),
    };
    let mut dialog = dialogs::storage_dialog(kind, path.into());
    if matches!(dialog.kind, Form::VolumeGroup)
        && let Some(node) = node.filter(|n| !n.group.is_empty())
    {
        dialog.fields[1] = node.group.clone();
    }
    dialog
}
fn notice(ui: &mut Ui, workspace: &mut Workspace) {
    let Some(message) = workspace.storage.notice.as_deref() else {
        return;
    };
    let mut close = false;
    egui::Modal::new(ui.make_persistent_id("storage-error")).show(ui.ctx(), |ui| {
        ui.set_width(430.);
        ui.heading("存储操作失败");
        egui::ScrollArea::vertical()
            .max_height(260.)
            .show(ui, |ui| {
                ui.add(egui::Label::new(message).wrap().selectable(true));
            });
        let button = ui.button("确定");
        if std::mem::take(&mut workspace.storage.notice_focus) {
            button.request_focus();
        }
        close = button.clicked()
            || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    });
    if close {
        workspace.storage.notice = None;
    }
}
