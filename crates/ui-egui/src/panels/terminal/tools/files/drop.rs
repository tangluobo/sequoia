use super::*;
pub(super) fn receive(
    ui: &mut Ui,
    workspace: &mut Workspace,
    connection: &tomato_core::Connection,
    bounds: egui::Rect,
    directory: String,
) {
    if ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new("file-drop-preview-installed")))
        .unwrap_or(false)
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("file-drop-view-bounds"), bounds));
    }
    let point = ui.input(|input| input.pointer.interact_pos());
    if !ui.is_enabled()
        || workspace.dialog.is_some()
        || point.is_none_or(|point| {
            !bounds.contains(point)
                || ui
                    .ctx()
                    .layer_id_at(point)
                    .is_some_and(|layer| layer != ui.layer_id())
        })
    {
        return;
    }
    let files = ui.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
    if files.is_empty() {
        return;
    }
    // Raw input is replayed when egui discards a layout pass; one OS drop is one job.
    let consumed = egui::Id::new(("file-drop-consumed", ui.ctx().viewport_id()));
    let frame = ui.ctx().cumulative_frame_nr();
    if ui.ctx().data(|data| data.get_temp::<u64>(consumed)) == Some(frame) {
        return;
    }
    ui.ctx().data_mut(|data| data.insert_temp(consumed, frame));
    if files.len() > 1000 {
        workspace.panes.entry(Section::Files).or_default().status =
            "一次最多上传 1000 个文件或目录".into();
        return;
    }
    if bucket_icon(connection, &directory).is_some() {
        super::super::dialogs::notice(workspace, "请先进入一个 Bucket 再上传文件");
        return;
    }
    let paths = files
        .iter()
        .map(|file| file.path())
        .filter(|path| !path.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .collect();
    workspace.upload_files(ui.ctx(), connection, directory, paths, files);
}
