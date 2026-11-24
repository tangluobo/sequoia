//! Shared Java SessionConfigDialog, preserving unedited connection fields.
use super::rdp_windows::{global_mode, independent};
use super::*;
#[cfg(test)]
mod tests;
pub(crate) struct Draft {
    tab: String,
    connection: String,
    name: String,
    scrollback: String,
    file_mode: String,
    mode: String,
    rdp: bool,
    inherited: [bool; 3],
    editor: crate::tabs::text_state::Editor,
    error: Option<String>,
}
impl TomatoApp {
    pub(in crate::app) fn close_session_dialog(&mut self, id: &str) {
        if self
            .session_config
            .as_ref()
            .is_some_and(|draft| draft.tab == id)
            && let Some(draft) = self.session_config.take()
        {
            draft.editor.release(&self.context);
            self.context.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of(("rdp-session", &draft.tab)),
                egui::ViewportCommand::Close,
            );
        }
    }
    pub(super) fn open_session_config(&mut self, id: &str) {
        let Some(Tab {
            panel: Panel::NativeDesktop { connection, .. } | Panel::Terminal { connection, .. },
            ..
        }) = self.tabs.iter().find(|tab| tab.id == id)
        else {
            return;
        };
        if let Some(old) = self.session_config.take() {
            old.editor.release(&self.context);
        }
        self.session_config = Some(Draft {
            tab: id.into(),
            connection: connection.id.clone(),
            name: connection.name.clone(),
            scrollback: connection
                .options
                .get("scrollbackLines")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(self.settings.scrollback_lines as i64)
                .to_string(),
            file_mode: connection
                .options
                .get("defaultFileViewMode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&self.settings.ssh_default_file_view_mode)
                .into(),
            rdp: connection.kind() == "RDP",
            mode: if independent(connection, &self.settings) {
                "WINDOW"
            } else {
                "TAB"
            }
            .into(),
            inherited: [
                connection
                    .options
                    .get("scrollbackLines")
                    .is_none_or(serde_json::Value::is_null),
                connection
                    .options
                    .get("defaultFileViewMode")
                    .is_none_or(serde_json::Value::is_null),
                tomato_core::java_text::is_blank(connection.text("rdpOpenMode")),
            ],
            editor: Default::default(),
            error: None,
        });
    }
    pub(in crate::app) fn session_config_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.session_config.take() else {
            return;
        };
        let mut result = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(("rdp-session", &draft.tab)),
            egui::ViewportBuilder::default()
                .with_title(format!("会话配置 - {}", draft.name))
                .with_inner_size([380., if draft.rdp { 195. } else { 160. }])
                .with_resizable(false),
            |ui, _| {
                let bounds = ui.max_rect();
                ui.painter().rect_filled(bounds, 0, Color32::from_gray(244));
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(bounds.shrink2(egui::vec2(20., 10.))),
                    |ui| {
                        ui.style_mut().override_font_id =
                            Some(crate::native_dialog::font(ctx, 12.));
                        ui.add_space(12.);
                        egui::Grid::new("session-settings")
                            .spacing([10., 10.])
                            .show(ui, |ui| {
                                ui.label("回滚行数:");
                                ui.add_enabled_ui(!draft.inherited[0], |ui| {
                                    let id = draft.editor.prepare(ui, "scrollback");
                                    ui.add(
                                        TextEdit::singleline(&mut draft.scrollback)
                                            .id(id)
                                            .desired_width(80.),
                                    );
                                });
                                if ui
                                    .checkbox(&mut draft.inherited[0], "使用全局配置")
                                    .changed()
                                {
                                    draft.scrollback = self.settings.scrollback_lines.to_string();
                                }
                                ui.end_row();
                                ui.label("默认文件视图:");
                                ui.add_enabled_ui(!draft.inherited[1], |ui| {
                                    mode_combo(
                                        ui,
                                        "file-mode",
                                        &mut draft.file_mode,
                                        &[
                                            ("ICON", "图标视图"),
                                            ("LIST", "详细列表"),
                                            ("COLUMN", "多列列表"),
                                        ],
                                    );
                                });
                                if ui
                                    .checkbox(&mut draft.inherited[1], "使用全局配置")
                                    .changed()
                                {
                                    draft.file_mode =
                                        self.settings.ssh_default_file_view_mode.clone();
                                }
                                ui.end_row();
                                if draft.rdp {
                                    ui.label("打开方式:");
                                    ui.add_enabled_ui(!draft.inherited[2], |ui| {
                                        mode_combo(
                                            ui,
                                            "rdp-mode",
                                            &mut draft.mode,
                                            &[("WINDOW", "独立窗口"), ("TAB", "连接标签页")],
                                        );
                                    });
                                    if ui
                                        .checkbox(&mut draft.inherited[2], "使用全局配置")
                                        .changed()
                                    {
                                        draft.mode = if global_mode(&self.settings)
                                            .eq_ignore_ascii_case("TAB")
                                        {
                                            "TAB"
                                        } else {
                                            "WINDOW"
                                        }
                                        .into();
                                    }
                                    ui.end_row();
                                }
                            });
                        if let Some(error) = &draft.error {
                            ui.colored_label(Color32::RED, error);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("取消").clicked() {
                                result = Some(false);
                            }
                            if ui.button("确定").clicked() {
                                result = Some(true);
                            }
                        });
                        if ui.input(|input| {
                            input.viewport().close_requested()
                                || input.key_pressed(egui::Key::Escape)
                        }) {
                            result = Some(false);
                        }
                    },
                );
            },
        );
        if result == Some(false) {
            draft.editor.release(ctx);
            return;
        }
        if result == Some(true) {
            let mut next = self.store.clone();
            if let Some(connection) = next
                .connections
                .iter_mut()
                .find(|c| c.id == draft.connection)
            {
                let values = draft.values();
                for (key, value) in &values {
                    connection.options.insert((*key).into(), value.clone());
                }
                if self.persist_store(next) {
                    for tab in &mut self.tabs {
                        if let Panel::NativeDesktop { connection, .. }
                        | Panel::Terminal { connection, .. } = &mut tab.panel
                            && connection.id == draft.connection
                        {
                            for (key, value) in &values {
                                connection.options.insert((*key).into(), value.clone());
                            }
                            terminal_config::apply(tab, &self.settings);
                        }
                    }
                    draft.editor.release(ctx);
                    return;
                }
                draft.error = self.error.take();
            } else {
                draft.error = Some("连接已被删除，请关闭此对话框。".into());
            }
        }
        self.session_config = Some(draft);
    }
}
impl Draft {
    fn values(&self) -> Vec<(&'static str, serde_json::Value)> {
        let value = |index, custom| {
            if self.inherited[index] {
                serde_json::Value::Null
            } else {
                custom
            }
        };
        let mut values = vec![
            (
                "scrollbackLines",
                value(
                    0,
                    self.scrollback
                        .trim()
                        .parse::<i32>()
                        .ok()
                        .map(|n| serde_json::json!(n.max(0)))
                        .unwrap_or_default(),
                ),
            ),
            (
                "defaultFileViewMode",
                value(1, serde_json::json!(self.file_mode)),
            ),
        ];
        if self.rdp {
            values.push(("rdpOpenMode", value(2, serde_json::json!(self.mode))));
        }
        values
    }
}
fn mode_combo(ui: &mut Ui, id: &str, value: &mut String, choices: &[(&str, &str)]) {
    let label = choices
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| *label)
        .unwrap_or(choices[0].1);
    egui::ComboBox::from_id_salt(id)
        .width(95.)
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (key, label) in choices {
                ui.selectable_value(value, (*key).into(), *label);
            }
        });
}
