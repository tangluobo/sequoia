use super::*;

pub(super) fn share(ui: &mut Ui, value: &mut SharedDirectory, users: &mut String) {
    egui::Grid::new("server-share-fields")
        .spacing([10., 12.])
        .show(ui, |ui| {
            ui.label("别名：");
            field(ui, &mut value.alias, "如 files, share, public", 280., false);
            ui.end_row();
            ui.label("本地路径：");
            directory(
                ui,
                &mut value.path,
                "选择要共享的本地目录",
                "选择目录",
                220.,
            );
            ui.end_row();
            ui.label("权限：");
            ui.checkbox(&mut value.read_only, "只读（禁止写入、删除、新建）");
            ui.end_row();
            ui.label("允许用户：");
            field(ui, users, "逗号分隔，留空表示所有账号", 280., false);
            ui.end_row();
        });
}
pub(super) fn account(ui: &mut Ui, value: &mut ServerAccount) {
    egui::Grid::new("server-account-fields")
        .spacing([10., 12.])
        .show(ui, |ui| {
            ui.label("用户名：");
            field(ui, &mut value.username, "", 240., false);
            ui.end_row();
            ui.label("密码：");
            field(ui, &mut value.password, "", 240., true);
            ui.end_row();
            ui.label("启用账号：");
            ui.checkbox(&mut value.enabled, "启用该账号（取消后无法登录）");
            ui.end_row();
            ui.label("主目录：");
            directory(
                ui,
                &mut value.home_directory,
                "留空表示使用默认根目录/共享目录",
                "选择用户主目录",
                180.,
            );
            ui.end_row();
        });
}
fn field(ui: &mut Ui, value: &mut String, hint: &str, width: f32, password: bool) {
    ui.add_sized(
        [width, 23.],
        TextEdit::singleline(value)
            .hint_text(hint)
            .password(password),
    );
}
fn directory(ui: &mut Ui, path: &mut String, hint: &str, title: &str, width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        field(ui, path, hint, width, false);
        if ui
            .add_sized(
                [38., 23.],
                egui::Button::new(RichText::new("浏览").size(11.)),
            )
            .clicked()
        {
            let mut dialog = rfd::FileDialog::new().set_title(title);
            if std::path::Path::new(path).is_dir() {
                dialog = dialog.set_directory(&*path);
            }
            if let Some(selected) = dialog.pick_folder() {
                *path = selected.display().to_string();
            }
        }
    });
}
