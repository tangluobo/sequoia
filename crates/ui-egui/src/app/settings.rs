//! Java settings sub-tabs, including the SSH terminal page selected from a tab.
use super::*;

const GRAYSCALE_TEXT: &str = "uiGrayscaleText";
pub(super) fn grayscale_text(settings: &SettingsState) -> bool {
    settings
        .extra
        .get(GRAYSCALE_TEXT)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    System,
    Ssh,
    Rdp,
}
#[cfg(test)]
mod tests;
#[derive(Clone)]
pub(super) struct Form {
    sidebar_visible: bool,
    grayscale_text: bool,
    scrollback_lines: usize,
    ssh_terminal_font_name: String,
    ssh_terminal_font_size: f32,
    ssh_service_management_enabled: bool,
    ssh_default_file_view_mode: String,
    rdp_open_mode: String,
}
impl From<&SettingsState> for Form {
    fn from(settings: &SettingsState) -> Self {
        Self {
            sidebar_visible: settings.sidebar_visible,
            grayscale_text: grayscale_text(settings),
            scrollback_lines: settings.scrollback_lines,
            ssh_terminal_font_name: settings.ssh_terminal_font_name.clone(),
            ssh_terminal_font_size: settings.ssh_terminal_font_size,
            ssh_service_management_enabled: settings.ssh_service_management_enabled,
            ssh_default_file_view_mode: settings.ssh_default_file_view_mode.clone(),
            rdp_open_mode: super::rdp_windows::global_mode(settings).into(),
        }
    }
}
impl Form {
    fn apply(&self, next: &mut SettingsState, page: Page) {
        match page {
            Page::System => {
                next.sidebar_visible = self.sidebar_visible;
                if self.grayscale_text != grayscale_text(next) {
                    next.extra
                        .insert(GRAYSCALE_TEXT.into(), self.grayscale_text.into());
                }
            }
            Page::Rdp => {
                next.extra
                    .insert("rdpOpenMode".into(), self.rdp_open_mode.clone().into());
            }
            Page::Ssh => {
                next.scrollback_lines = self.scrollback_lines;
                next.ssh_terminal_font_name = if self.ssh_terminal_font_name.trim().is_empty() {
                    "monospace".into()
                } else {
                    self.ssh_terminal_font_name.clone()
                };
                next.ssh_terminal_font_size = self.ssh_terminal_font_size;
                next.ssh_service_management_enabled = self.ssh_service_management_enabled;
                next.ssh_default_file_view_mode = self.ssh_default_file_view_mode.clone();
            }
        }
    }
}
impl TomatoApp {
    pub(super) fn settings_view(&mut self, ui: &mut Ui) {
        crate::native_tabs::show(
            ui,
            "settings-pages",
            &mut self.settings_page,
            crate::native_tabs::Style::Compact,
            [
                (Page::System, "系统设置"),
                (Page::Ssh, "SSH终端"),
                (Page::Rdp, "远程桌面"),
            ],
        );
        let mut form = self
            .settings_form
            .take()
            .unwrap_or_else(|| Box::new(Form::from(&self.settings)));
        let page = self.settings_page;
        let mut sidebar_changed = false;
        let save = egui::Frame::NONE
            .inner_margin(20)
            .show(ui, |ui| match page {
                Page::Ssh => Self::ssh_settings(ui, &mut form),
                Page::System => {
                    ui.label(RichText::new("系统设置").size(20.).strong());
                    ui.add_space(20.);
                    sidebar_changed = ui
                        .checkbox(&mut form.sidebar_visible, "开启侧边栏")
                        .changed();
                    if crate::font_assets::lcd_enabled(ui.ctx()) {
                        ui.checkbox(&mut form.grayscale_text, "灰阶文字抗锯齿")
                            .on_hover_text(
                                "使用 Windows 原生灰阶字形，消除文字边缘的彩色晕边；保存后立即生效",
                            );
                    }
                    ui.add_space(15.);
                    ui.add(crate::theme::green_button("保存设置")).clicked()
                }
                Page::Rdp => {
                    ui.label(RichText::new("远程桌面").size(20.).strong());
                    ui.add_space(20.);
                    ui.horizontal(|ui| {
                        ui.label("打开方式");
                        mode_combo(
                            ui,
                            "rdp-open-mode-global",
                            &mut form.rdp_open_mode,
                            &[("WINDOW", "独立窗口"), ("TAB", "连接标签页")],
                        );
                    });
                    ui.add(crate::theme::green_button("应用并保存")).clicked()
                }
            })
            .inner;
        if sidebar_changed && !self.save_sidebar_visibility(form.sidebar_visible) {
            form.sidebar_visible = self.settings.sidebar_visible;
        }
        if save && self.save_settings(&form, page) && page == Page::Ssh {
            form.ssh_terminal_font_name = self.settings.ssh_terminal_font_name.clone();
        }
        self.settings_form = Some(form);
    }
    fn ssh_settings(ui: &mut Ui, form: &mut Form) -> bool {
        ui.label(RichText::new("SSH终端").size(20.).strong());
        ui.add_space(15.);
        ui.horizontal_top(|ui|{
            egui::Grid::new("ssh-settings").spacing([10.,12.]).show(ui,|ui|{
                ui.label("字体名称");
                ui.horizontal(|ui|{
                    ui.spacing_mut().item_spacing.x=0.;
                    let editor=ui.add(TextEdit::singleline(&mut form.ssh_terminal_font_name).desired_width(190.));
                    let arrow=ui.scope(|ui|{
                        ui.spacing_mut().button_padding.y=0.;ui.spacing_mut().interact_size.y=editor.rect.height();
                        ui.add_sized([24.,editor.rect.height()],egui::Button::new("▾"))
                    }).inner;
                    let anchor=editor.rect.union(arrow.rect);
                    egui::Popup::menu(&arrow).anchor(anchor).width(anchor.width()).align(egui::RectAlign::BOTTOM_START).align_alternatives(&[]).show(|ui|{
                        ui.set_min_width(ui.available_width());
                        ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Extend);
                        ui.spacing_mut().item_spacing.y=0.;
                        ui.spacing_mut().interact_size.y=22.;
                        egui::ScrollArea::vertical().max_height(ui.spacing().combo_height).show(ui,|ui|{
                            for name in crate::font_assets::terminal::names(ui.ctx()).iter(){ui.selectable_value(&mut form.ssh_terminal_font_name,name.clone(),name);}
                        });
                    });
                });ui.end_row();
                ui.label("字体大小");ui.add(egui::DragValue::new(&mut form.ssh_terminal_font_size).range(6.0..=48.));ui.end_row();
                ui.label("回滚行数");ui.add(egui::DragValue::new(&mut form.scrollback_lines).range(0..=i32::MAX as usize));ui.end_row();
                ui.label("");ui.checkbox(&mut form.ssh_service_management_enabled,"开启服务管理");ui.end_row();
                ui.label("默认文件视图");mode_combo(ui,"ssh-file-view",&mut form.ssh_default_file_view_mode,&[("ICON","图标视图"),("LIST","详细列表"),("COLUMN","多列列表")]);ui.end_row();
            });
            ui.add_space(30.);
            ui.vertical(|ui|{
                ui.label("预览：");
                ui.label(RichText::new("abcdefghijklmnopqrstuvwxyz 0123456789\nABCDEFGHIJKLMNOPQRSTUVWXYZ ~!@#$%^&*()")
                    .font(crate::font_assets::terminal::preview(ui.ctx(),&form.ssh_terminal_font_name,form.ssh_terminal_font_size)));
            });
        });
        ui.add_space(15.);
        ui.add(crate::theme::green_button("应用并保存")).clicked()
    }
    pub(super) fn save_settings(&mut self, form: &Form, page: Page) -> bool {
        let mut next = self.settings.clone();
        form.apply(&mut next, page);
        self.persist_settings(next, page)
    }
    fn save_sidebar_visibility(&mut self, visible: bool) -> bool {
        let mut next = self.settings.clone();
        next.sidebar_visible = visible;
        self.persist_settings(next, Page::System)
    }
    fn persist_settings(&mut self, next: SettingsState, page: Page) -> bool {
        match self.preferences.save(&next) {
            Ok(()) => {
                if grayscale_text(&self.settings) != grayscale_text(&next) {
                    crate::font_assets::set_grayscale(&self.context, grayscale_text(&next));
                }
                self.settings = next;
                self.collaboration_personal_changed();
                self.status = "全局设置已保存（原版 global.json）".into();
                if page == Page::Ssh {
                    if !self.settings.ssh_service_management_enabled {
                        self.ssh_hosts.clear();
                    }
                    crate::font_assets::terminal::ensure(
                        &self.context,
                        &self.settings.ssh_terminal_font_name,
                    );
                    for tab in &mut self.tabs {
                        terminal_config::apply_runtime(tab, &self.settings);
                    }
                }
                true
            }
            Err(error) => {
                self.error = Some(format!("保存全局设置失败：{error:#}"));
                false
            }
        }
    }
}
fn mode_combo(ui: &mut Ui, id: &str, value: &mut String, choices: &[(&str, &str)]) {
    let label = choices
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| *label)
        .unwrap_or(choices[0].1);
    egui::ComboBox::from_id_salt(id)
        .width(150.)
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (key, label) in choices {
                ui.selectable_value(value, (*key).into(), *label);
            }
        });
}
