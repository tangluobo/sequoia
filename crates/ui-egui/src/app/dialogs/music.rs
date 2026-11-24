use crate::panels::music::DOWNLOAD_DIRECTORY;
use egui::{TextEdit, Ui};
use tomato_core::Connection;

pub(super) fn is_music(draft: &Connection) -> bool {
    draft.kind() == "TOOL" && draft.text("toolType") == "music_downloader"
}

pub(super) fn directory(draft: &Connection) -> &str {
    draft.text(DOWNLOAD_DIRECTORY)
}

pub(super) fn fields(ui: &mut Ui, draft: &mut Connection) {
    ui.label("下载目录：");
    let mut path = directory(draft).to_owned();
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut path)
                .hint_text("选择文件夹")
                .desired_width(280.0),
        );
        if ui.button("浏览…").clicked() {
            let mut dialog = rfd::FileDialog::new().set_title("选择音乐下载目录");
            if !path.trim().is_empty() {
                dialog = dialog.set_directory(path.trim());
            }
            if let Some(folder) = dialog.pick_folder() {
                path = folder.to_string_lossy().into_owned();
            }
        }
    });
    draft.set(DOWNLOAD_DIRECTORY, path);
    ui.end_row();
}
