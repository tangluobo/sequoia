//! Database-tab settings and refresh commands use the existing editor/controller paths.
use super::*;
use tomato_core::preferences::TableFont;
mod fonts;
#[cfg(test)]
mod tests;

pub(super) struct Dialog {
    pub font: TableFont,
    picker: Option<fonts::Picker>,
}
impl Dialog {
    pub(super) fn new(font: TableFont) -> Self {
        Self { font, picker: None }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Configure,
    RefreshDesign,
    RefreshData,
}
pub(super) fn menu(ui: &mut Ui, tab: &Tab) -> Option<Action> {
    let Panel::Sql { workbench, .. } = &tab.panel else {
        return None;
    };
    let designer = workbench.designer.is_some() || workbench.structure;
    if !designer && workbench.table.is_none() {
        return None;
    }
    let mut action = None;
    if ui.add(crate::compact_menu::Item::new("表格配置")).clicked() {
        action = Some(Action::Configure);
        ui.close();
    }
    if ui
        .add_enabled(
            !tab.busy,
            crate::compact_menu::Item::new(if designer {
                "刷新结构"
            } else {
                "刷新数据"
            }),
        )
        .clicked()
    {
        action = Some(if designer {
            Action::RefreshDesign
        } else {
            Action::RefreshData
        });
        ui.close();
    }
    action
}
impl TomatoApp {
    pub(super) fn table_tab_action(&mut self, id: &str, action: Action) {
        if !matches!(action, Action::Configure) && self.load_object_tab(id) {
            if matches!(action, Action::RefreshData) {
                self.active = Some(id.to_owned());
            }
            return;
        }
        match action {
            Action::Configure => {
                self.table_config = Some(Box::new(Dialog::new(self.settings.table_font())))
            }
            Action::RefreshDesign => {
                let readonly = self
                    .tabs
                    .iter()
                    .find(|t| t.id == id && !t.busy)
                    .and_then(|tab| {
                        if let Panel::Sql {
                            connection,
                            workbench,
                            ..
                        } = &tab.panel
                            && workbench.structure
                            && workbench.designer.is_none()
                        {
                            workbench
                                .table
                                .clone()
                                .map(|table| (connection.clone(), table))
                        } else {
                            None
                        }
                    });
                if let Some((connection, table)) = readonly {
                    self.submit(
                        id,
                        Command::Database {
                            connection,
                            action: tomato_core::workbench::DatabaseAction::Definition { table },
                        },
                        Purpose::Normal,
                    );
                } else {
                    self.refresh_design(id);
                }
            }
            Action::RefreshData => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id && !t.busy)
                    && let Panel::Sql { workbench, .. } = &mut tab.panel
                {
                    // The ordinary pending-page path commits editing and asks before losing changes.
                    workbench.pending_page = workbench.request();
                    self.active = Some(id.to_owned());
                }
            }
        }
    }
    pub(super) fn table_config_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.table_config.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let viewport = egui::ViewportId::from_hash_of("table-config-dialog");
        let capture = std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("table_config")
            && std::env::var_os("TOMATO_UI_CAPTURE").is_some();
        if capture {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
        }
        ctx.show_viewport_immediate(
            viewport,
            egui::ViewportBuilder::default()
                .with_title("表格配置")
                .with_inner_size([450., 96.])
                .with_resizable(false)
                .with_active(!capture),
            |ui, _| {
                let origin = ui.max_rect().min;
                ui.painter()
                    .rect_filled(ui.max_rect(), 0., Color32::from_gray(244));
                let font = crate::native_dialog::font(ui.ctx(), 12.);
                ui.style_mut().override_font_id = Some(font.clone());
                ui.painter().text(
                    origin + egui::vec2(20., 31.5),
                    egui::Align2::LEFT_CENTER,
                    "表格字体:",
                    font.clone(),
                    Color32::BLACK,
                );
                let combo =
                    egui::Rect::from_min_size(origin + egui::vec2(81., 20.), egui::vec2(150., 23.));
                fonts::combo(ui, combo, &mut draft.font.name, &mut draft.picker);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                        origin + egui::vec2(241., 20.),
                        egui::vec2(60., 23.),
                    )),
                    |ui| spinner(ui, &mut draft.font.size),
                );
                ui.painter().text(
                    origin + egui::vec2(311., 31.5),
                    egui::Align2::LEFT_CENTER,
                    "px",
                    font,
                    Color32::BLACK,
                );
                let button = |x| {
                    egui::Rect::from_min_size(origin + egui::vec2(x, 63.), egui::vec2(75., 23.))
                };
                save = crate::native_dialog::button(ui, button(280.), "确定", true).clicked();
                cancel = crate::native_dialog::button(ui, button(365.), "取消", false).clicked();
                open = !ui.input(|i| i.viewport().close_requested());
                if !egui::Popup::is_any_open(ctx) {
                    save |=
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    cancel |=
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                }
            },
        );
        if (save || cancel || !open) && draft.picker.take().is_some() {
            fonts::close(ctx);
        }
        if save {
            let mut next = self.settings.clone();
            match next
                .set_table_font(&draft.font)
                .and_then(|_| self.preferences.save(&next))
            {
                Ok(()) => {
                    self.settings = next;
                    crate::table_style::sync(ctx, draft.font.clone(), true);
                    self.collaboration_personal_changed();
                    self.status = "表格配置已保存".into();
                    ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
                    ctx.request_repaint();
                    return;
                }
                Err(error) => self.error = Some(format!("保存表格配置失败：{error:#}")),
            }
        }
        if open && !cancel {
            self.table_config = Some(draft);
        } else {
            ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
        }
    }
}

fn spinner(ui: &mut Ui, size: &mut u8) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(60., 23.), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    ui.painter().rect(
        rect,
        2.,
        egui::Color32::WHITE,
        egui::Stroke::new(1., egui::Color32::from_gray(180)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 5., rect.center().y),
        egui::Align2::LEFT_CENTER,
        size.to_string(),
        crate::native_dialog::font(ui.ctx(), 12.),
        egui::Color32::BLACK,
    );
    for (up, key) in [(true, egui::Key::ArrowUp), (false, egui::Key::ArrowDown)] {
        let y = if up { rect.top() } else { rect.center().y };
        let button =
            egui::Rect::from_min_size(egui::pos2(rect.right() - 15., y), egui::vec2(15., 11.5));
        let id = response.id.with(up);
        let r = ui.interact(button, id, egui::Sense::click());
        crate::native_dialog::paint_button(
            ui,
            button,
            crate::native_dialog::ButtonState {
                default: false,
                hover: r.hovered(),
                armed: r.is_pointer_button_down_on(),
                focused: false,
                enabled: ui.is_enabled(),
            },
        );
        let p = button.center();
        let d = if up { -1. } else { 1. };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                p + egui::vec2(-3., -d),
                p + egui::vec2(3., -d),
                p + egui::vec2(0., 2. * d),
            ],
            egui::Color32::from_gray(70),
            egui::Stroke::NONE,
        ));
        let step = crate::native_dialog::spinner::repeat(ui, &r, id)
            || (response.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)));
        if step {
            *size = if up {
                size.saturating_add(1).min(24)
            } else {
                size.saturating_sub(1).max(8)
            };
            response.request_focus();
        }
    }
}
