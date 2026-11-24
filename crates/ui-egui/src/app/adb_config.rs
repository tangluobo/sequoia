//! ADB connection settings and wireless pairing.
use super::*;
mod pairing;
#[cfg(test)]
mod tests;
pub(super) use pairing::Pairing;

pub(super) fn fields(ui: &mut Ui, draft: &mut Connection, port: &mut String) {
    ui.label("主机：");
    let mut host = draft.text("host").to_owned();
    if ui
        .add_sized(
            [320., 28.],
            TextEdit::singleline(&mut host)
                .vertical_align(egui::Align::Center)
                .hint_text("已开启无线调试的 Android 设备地址"),
        )
        .changed()
    {
        draft.set("host", host);
    }
    ui.end_row();
    ui.label("端口：");
    ui.add_sized(
        [120., 28.],
        TextEdit::singleline(port).vertical_align(egui::Align::Center),
    );
    ui.end_row();
    ui.label("");
    let mut fullscreen = draft.flag("fullscreen");
    if ui.checkbox(&mut fullscreen, "独立窗口打开").changed() {
        draft.set("fullscreen", fullscreen);
    }
    ui.end_row();
    ui.label("备注：");
    let mut description = draft.text("description").to_owned();
    if ui
        .add_sized(
            [320., 28.],
            TextEdit::singleline(&mut description)
                .vertical_align(egui::Align::Center)
                .hint_text("备注信息"),
        )
        .changed()
    {
        draft.set("description", description);
    }
    ui.end_row();
}
pub(super) fn normalize(draft: &mut Connection, port: &str) -> anyhow::Result<()> {
    if draft.kind() != "ADB" {
        return Ok(());
    }
    anyhow::ensure!(
        !draft.text("host").trim().is_empty(),
        "ADB 主机地址不能为空"
    );
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("ADB 端口必须为 1–65535"))?;
    anyhow::ensure!(port != 0, "ADB 端口必须为 1–65535");
    draft.name = draft.name.trim().to_owned();
    draft.set("port", port);
    for key in ["host", "description"] {
        draft.set(key, draft.text(key).trim().to_owned());
    }
    for key in ["username", "password", "remoteDesktopClientPath"] {
        draft.set(key, serde_json::Value::Null);
    }
    for key in ["usePassword", "useKey", "savePassword"] {
        draft.set(key, false);
    }
    draft.set("remoteDesktopTlsPort", 0);
    Ok(())
}
