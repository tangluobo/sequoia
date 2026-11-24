use super::*;
pub(super) fn fields(ui: &mut Ui, connection: &mut Connection, directory: bool) {
    if directory {
        for (key, label) in [("bucket", "目录(Bucket)："), ("s3Prefix", "子目录：")] {
            form_field(ui, connection, key, label, false);
        }
    }
    for (key, label, secret) in [
        ("endpoint", "端点：", false),
        ("region", "区域：", false),
        ("username", "Access Key：", false),
        ("password", "Secret Key：", true),
    ] {
        form_field(ui, connection, key, label, secret);
        if secret {
            ui.label("");
            check(ui, connection, "savePassword", "保存密钥");
            ui.end_row();
        }
    }
    ui.label("");
    check(
        ui,
        connection,
        "pathStyleAccess",
        "路径风格访问（Path-Style Access）",
    );
    ui.end_row();
    if !directory {
        form_field(ui, connection, "publicAccessUrl", "访问URL：", false);
    }
}
pub(super) fn directory(ui: &mut Ui, connection: &mut Connection) {
    ui.label("存储后端：");
    let mut kind = connection.text("directoryType").to_owned();
    if kind.is_empty() {
        kind = "LOCAL".into();
    }
    egui::ComboBox::from_id_salt("directory-storage-type")
        .selected_text(if kind == "S3" {
            "S3兼容存储"
        } else {
            "本地目录"
        })
        .show_ui(ui, |ui| {
            if ui
                .selectable_value(&mut kind, "LOCAL".into(), "本地目录")
                .changed()
            {
                connection.set("directoryType", kind.clone());
            }
            if ui
                .selectable_value(&mut kind, "S3".into(), "S3兼容存储")
                .changed()
            {
                connection.set("directoryType", kind.clone());
            }
        });
    ui.end_row();
    if kind == "S3" {
        fields(ui, connection, true);
    } else {
        ui.label("目录路径：");
        ui.horizontal(|ui| {
            let mut path = connection.text("localDirectoryPath").to_owned();
            if ui
                .add_sized(
                    [220., 28.],
                    TextEdit::singleline(&mut path).hint_text("选择或输入本地目录路径"),
                )
                .changed()
            {
                connection.set("localDirectoryPath", path.clone());
            }
            if ui.button("浏览").clicked()
                && let Some(selected) = rfd::FileDialog::new()
                    .set_title("选择本地目录")
                    .pick_folder()
            {
                connection.set("localDirectoryPath", selected.display().to_string());
            }
        });
        ui.end_row();
    }
}
